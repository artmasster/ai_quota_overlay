//! Minimal pure-Rust read-only SQLite reader: just enough to read rows of a rowid table keyed by its
//! first column (Cursor's `state.vscdb` → `ItemTable(key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)`).
//!
//! Reads the main file and, when present, the `-wal` file (latest committed frame of each page wins).
//! Files are opened with shared read access and never written. Corruption → `InvalidData`, never a panic.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Largest payload accepted for one row (SQLite's own hard limit is 2^31-1).
const MAX_PAYLOAD: u64 = 1 << 31;

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn open_shared(path: &Path) -> io::Result<File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: Cursor keeps the db open and writes to it.
        o.share_mode(7);
    }
    o.open(path)
}

/// Reads `len` bytes at `off`; a short file is corruption, not EOF.
fn read_at(f: &mut File, off: u64, buf: &mut [u8]) -> io::Result<()> {
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(buf).map_err(|e| if e.kind() == io::ErrorKind::UnexpectedEof { bad("file truncated") } else { e })
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// SQLite varint: 1–9 bytes, big-endian, 7 bits per byte except the 9th which carries 8.
fn varint(b: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let byte = *b.get(at + i)?;
        if i == 8 {
            return Some(((v << 8) | byte as u64, 9));
        }
        v = (v << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

fn valid_page_size(n: u32) -> bool {
    (512..=65536).contains(&n) && n.is_power_of_two()
}

// ---------------------------------------------------------------------------------------------------------
// WAL

/// Committed frames of a WAL file: page number → file offset of the page image.
struct Wal {
    file: File,
    page_size: usize,
    /// Database size in pages after the last commit.
    db_pages: u32,
    frames: HashMap<u32, u64>,
}

fn wal_cksum(data: &[u8], big_endian: bool, s: (u32, u32)) -> (u32, u32) {
    let (mut s0, mut s1) = s;
    for c in data.chunks_exact(8) {
        let (x0, x1) = if big_endian {
            (be32(c, 0), be32(c, 4))
        } else {
            (u32::from_le_bytes([c[0], c[1], c[2], c[3]]), u32::from_le_bytes([c[4], c[5], c[6], c[7]]))
        };
        s0 = s0.wrapping_add(x0).wrapping_add(s1);
        s1 = s1.wrapping_add(x1).wrapping_add(s0);
    }
    (s0, s1)
}

/// Parses a WAL file the way SQLite's recovery does: stops at the first frame whose salts or running
/// checksum don't match, and keeps only frames up to the last commit frame. None = no usable WAL content.
fn read_wal(mut file: File) -> io::Result<Option<Wal>> {
    let len = file.metadata()?.len();
    if len < 32 {
        return Ok(None);
    }
    let mut hdr = [0u8; 32];
    read_at(&mut file, 0, &mut hdr)?;
    let magic = be32(&hdr, 0);
    if magic & !1 != 0x377f_0682 || be32(&hdr, 4) != 3_007_000 {
        return Ok(None);
    }
    let big_endian = magic & 1 == 1;
    let page_size = be32(&hdr, 8);
    if !valid_page_size(page_size) {
        return Ok(None);
    }
    let (salt1, salt2) = (be32(&hdr, 16), be32(&hdr, 20));
    let mut s = wal_cksum(&hdr[..24], big_endian, (0, 0));
    if s != (be32(&hdr, 24), be32(&hdr, 28)) {
        return Ok(None);
    }

    let page_size = page_size as usize;
    let frame_size = 24 + page_size as u64;
    let mut frame = vec![0u8; 24 + page_size];
    let mut pending: Vec<(u32, u64)> = Vec::new();
    let mut frames = HashMap::new();
    let mut db_pages = 0u32;
    let mut off = 32u64;
    file.seek(SeekFrom::Start(off))?;
    while off + frame_size <= len {
        if file.read_exact(&mut frame).is_err() {
            break;
        }
        let pgno = be32(&frame, 0);
        let commit = be32(&frame, 4);
        if pgno == 0 || be32(&frame, 8) != salt1 || be32(&frame, 12) != salt2 {
            break;
        }
        let c = wal_cksum(&frame[..8], big_endian, s);
        let c = wal_cksum(&frame[24..], big_endian, c);
        if c != (be32(&frame, 16), be32(&frame, 20)) {
            break;
        }
        s = c;
        pending.push((pgno, off + 24));
        if commit != 0 {
            frames.extend(pending.drain(..));
            db_pages = commit;
        }
        off += frame_size;
    }
    if db_pages == 0 {
        return Ok(None);
    }
    Ok(Some(Wal { file, page_size, db_pages, frames }))
}

// ---------------------------------------------------------------------------------------------------------
// Pager

struct Pager {
    file: File,
    file_len: u64,
    page_size: usize,
    /// Page size minus the reserved bytes at the end of each page.
    usable: usize,
    page_count: u32,
    /// 1 = UTF-8, 2 = UTF-16le, 3 = UTF-16be.
    text_encoding: u32,
    wal: Option<Wal>,
}

fn wal_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push("-wal");
    PathBuf::from(s)
}

impl Pager {
    /// None = an empty database (0-byte file).
    fn open(path: &Path, use_wal: bool) -> io::Result<Option<Pager>> {
        let mut file = open_shared(path)?;
        let file_len = file.metadata()?.len();
        if file_len == 0 {
            // SQLite treats a WAL next to an empty database as stale.
            return Ok(None);
        }
        let wal = if use_wal {
            match open_shared(&wal_path(path)) {
                Ok(f) => read_wal(f)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            }
        } else {
            None
        };

        let mut hdr = [0u8; 100];
        let page_size = if file_len >= 100 {
            read_at(&mut file, 0, &mut hdr)?;
            if &hdr[..16] != b"SQLite format 3\0" {
                return Err(bad("not a SQLite database"));
            }
            let raw = be16(&hdr, 16) as u32;
            let ps = if raw == 1 { 65536 } else { raw };
            if !valid_page_size(ps) {
                return Err(bad("bad page size"));
            }
            if let Some(w) = &wal {
                if w.page_size != ps as usize {
                    return Err(bad("WAL page size differs from database"));
                }
            }
            ps as usize
        } else {
            return Err(bad("file too short"));
        };

        let page_count = match &wal {
            Some(w) => w.db_pages,
            None => {
                let in_file = u32::try_from(file_len / page_size as u64).map_err(|_| bad("file too large"))?;
                let in_header = be32(&hdr, 28);
                if in_header != 0 && be32(&hdr, 24) == be32(&hdr, 92) {
                    in_header.min(in_file)
                } else {
                    in_file
                }
            }
        };

        let mut pager = Pager { file, file_len, page_size, usable: page_size, page_count, text_encoding: 1, wal };
        // Page 1 (possibly from the WAL) holds the authoritative header.
        let p1 = pager.page(1)?;
        if &p1[..16] != b"SQLite format 3\0" {
            return Err(bad("not a SQLite database"));
        }
        let raw = be16(&p1, 16) as u32;
        if (if raw == 1 { 65536 } else { raw }) as usize != page_size {
            return Err(bad("page size mismatch"));
        }
        let reserved = p1[20] as usize;
        if page_size - reserved < 480 {
            return Err(bad("bad reserved space"));
        }
        pager.usable = page_size - reserved;
        pager.text_encoding = match be32(&p1, 56) {
            0 | 1 => 1,
            e @ (2 | 3) => e,
            _ => return Err(bad("bad text encoding")),
        };
        Ok(Some(pager))
    }

    fn page(&mut self, pgno: u32) -> io::Result<Vec<u8>> {
        if pgno == 0 || pgno > self.page_count {
            return Err(bad(format!("page {pgno} out of range")));
        }
        let mut buf = vec![0u8; self.page_size];
        if let Some(w) = &mut self.wal {
            if let Some(&off) = w.frames.get(&pgno) {
                read_at(&mut w.file, off, &mut buf)?;
                return Ok(buf);
            }
        }
        let off = (pgno as u64 - 1) * self.page_size as u64;
        if off + self.page_size as u64 > self.file_len {
            return Err(bad(format!("page {pgno} past end of file")));
        }
        read_at(&mut self.file, off, &mut buf)?;
        Ok(buf)
    }

    /// Bytes of a leaf cell's payload kept on the b-tree page itself.
    fn local_size(&self, p: u64) -> u64 {
        let u = self.usable as u64;
        let x = u - 35;
        if p <= x {
            return p;
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((p - m) % (u - 4));
        if k <= x {
            k
        } else {
            m
        }
    }

    /// Reads `len` payload bytes starting at `start`, following the overflow chain when needed.
    fn payload(&mut self, cell: &Cell, start: u64, len: u64) -> io::Result<Vec<u8>> {
        let end = start.checked_add(len).ok_or_else(|| bad("payload range"))?;
        if end > cell.size {
            return Err(bad("record field past end of payload"));
        }
        let local = cell.local.len() as u64;
        if end <= local {
            return Ok(cell.local[start as usize..end as usize].to_vec());
        }
        let mut out = Vec::with_capacity(len.min(1 << 20) as usize);
        if start < local {
            out.extend_from_slice(&cell.local[start as usize..]);
        }
        let chunk = (self.usable - 4) as u64;
        let max_pages = (cell.size - local).div_ceil(chunk);
        let mut pos = local;
        let mut pgno = cell.overflow;
        let mut seen = HashSet::new();
        while pos < end {
            if pgno == 0 || seen.len() as u64 >= max_pages {
                return Err(bad("overflow chain too short"));
            }
            if !seen.insert(pgno) {
                return Err(bad("overflow chain cycle"));
            }
            let page = self.page(pgno)?;
            let next = be32(&page, 0);
            let data = &page[4..self.usable];
            let (lo, hi) = (pos, pos + chunk);
            if hi > start {
                let a = start.max(lo) - lo;
                let b = end.min(hi) - lo;
                out.extend_from_slice(&data[a as usize..b as usize]);
            }
            pos = hi;
            pgno = next;
        }
        Ok(out)
    }

    /// Visits every leaf cell of the table b-tree rooted at `root`, in rowid order; `f` returns false to stop.
    fn walk(&mut self, root: u32, f: &mut dyn FnMut(&mut Pager, &Cell) -> io::Result<bool>) -> io::Result<()> {
        let mut stack = vec![root];
        let mut visited = HashSet::new();
        while let Some(pgno) = stack.pop() {
            if !visited.insert(pgno) || visited.len() > self.page_count as usize {
                return Err(bad("b-tree cycle"));
            }
            let page = self.page(pgno)?;
            let h = if pgno == 1 { 100 } else { 0 };
            let kind = page[h];
            let ncells = be16(&page, h + 3) as usize;
            let (ptrs, interior) = match kind {
                0x05 => (h + 12, true),
                0x0D => (h + 8, false),
                _ => return Err(bad(format!("page {pgno}: not a table b-tree page ({kind:#04x})"))),
            };
            let usable = self.usable;
            if ptrs + 2 * ncells > usable {
                return Err(bad("cell count too large"));
            }
            let cell_at = |i: usize| -> io::Result<usize> {
                let p = be16(&page, ptrs + 2 * i) as usize;
                if p < ptrs + 2 * ncells || p >= usable {
                    return Err(bad("cell pointer out of range"));
                }
                Ok(p)
            };
            if interior {
                let mut children = Vec::with_capacity(ncells + 1);
                for i in 0..ncells {
                    let p = cell_at(i)?;
                    if p + 4 > usable {
                        return Err(bad("interior cell out of range"));
                    }
                    children.push(be32(&page, p));
                }
                children.push(be32(&page, h + 8));
                stack.extend(children.into_iter().rev());
                continue;
            }
            for i in 0..ncells {
                let p = cell_at(i)?;
                let area = &page[..usable];
                let (size, n1) = varint(area, p).ok_or_else(|| bad("cell header out of range"))?;
                let (rowid, n2) = varint(area, p + n1).ok_or_else(|| bad("cell header out of range"))?;
                if size > MAX_PAYLOAD || size > self.page_count as u64 * usable as u64 {
                    return Err(bad("payload too large"));
                }
                let local = self.local_size(size) as usize;
                let body = p + n1 + n2;
                if body + local > usable {
                    return Err(bad("cell payload out of range"));
                }
                let overflow = if (local as u64) < size {
                    if body + local + 4 > usable {
                        return Err(bad("cell overflow pointer out of range"));
                    }
                    be32(&page, body + local)
                } else {
                    0
                };
                let cell = Cell { size, rowid: rowid as i64, local: page[body..body + local].to_vec(), overflow };
                if !f(self, &cell)? {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Serial types and byte offsets of a record's columns.
    fn record_header(&mut self, cell: &Cell) -> io::Result<Vec<(u64, u64)>> {
        let first = self.payload(cell, 0, cell.size.min(9))?;
        let (hlen, n) = varint(&first, 0).ok_or_else(|| bad("record header"))?;
        if hlen > cell.size || (hlen as usize) < n {
            return Err(bad("record header size"));
        }
        let hdr = self.payload(cell, 0, hlen)?;
        let mut at = n;
        let mut off = hlen;
        let mut cols = Vec::new();
        while at < hdr.len() {
            let (t, k) = varint(&hdr, at).ok_or_else(|| bad("record header"))?;
            at += k;
            cols.push((t, off));
            off = off.checked_add(serial_len(t)?).ok_or_else(|| bad("record size"))?;
        }
        if off > cell.size {
            return Err(bad("record body past payload"));
        }
        Ok(cols)
    }

    fn column(&mut self, cell: &Cell, cols: &[(u64, u64)], i: usize) -> io::Result<Val> {
        let Some(&(t, off)) = cols.get(i) else { return Ok(Val::Null) };
        let len = serial_len(t)?;
        let b = self.payload(cell, off, len)?;
        Ok(match t {
            0 => Val::Null,
            1..=6 => {
                let mut v: i64 = if b[0] & 0x80 != 0 { -1 } else { 0 };
                for &x in &b {
                    v = (v << 8) | x as i64;
                }
                Val::Int(v)
            }
            7 => Val::Float(f64::from_be_bytes(b.try_into().map_err(|_| bad("float"))?)),
            8 => Val::Int(0),
            9 => Val::Int(1),
            t if t >= 12 && t % 2 == 0 => Val::Blob(b),
            _ => Val::Text(self.utf8(b)),
        })
    }

    fn utf8(&self, b: Vec<u8>) -> Vec<u8> {
        if self.text_encoding == 1 {
            return b;
        }
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| if self.text_encoding == 2 { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units).into_bytes()
    }

    /// Root page of the named table, from sqlite_master (rooted at page 1).
    fn table_root(&mut self, name: &str) -> io::Result<Option<u32>> {
        let mut root = None;
        self.walk(1, &mut |pg, cell| {
            let cols = pg.record_header(cell)?;
            let is_table = matches!(pg.column(cell, &cols, 0)?, Val::Text(t) if t == b"table");
            if !is_table {
                return Ok(true);
            }
            if let Val::Text(n) = pg.column(cell, &cols, 1)? {
                if n.eq_ignore_ascii_case(name.as_bytes()) {
                    if let Val::Int(r) = pg.column(cell, &cols, 3)? {
                        root = Some(u32::try_from(r).map_err(|_| bad("bad root page"))?);
                        return Ok(false);
                    }
                    return Err(bad("table has no root page"));
                }
            }
            Ok(true)
        })?;
        Ok(root)
    }
}

struct Cell {
    /// Total payload size.
    size: u64,
    rowid: i64,
    local: Vec<u8>,
    /// First overflow page, 0 if none.
    overflow: u32,
}

enum Val {
    Null,
    Int(i64),
    Float(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

fn serial_len(t: u64) -> io::Result<u64> {
    Ok(match t {
        0 | 8 | 9 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 6,
        6 | 7 => 8,
        10 | 11 => return Err(bad("reserved serial type")),
        t => (t - 12) / 2,
    })
}

/// Reads `keys` from a `(key, value)` table; values are the stored bytes (TEXT as UTF-8, BLOB as-is,
/// numbers as their decimal text). Keys that are missing or NULL are left out of the map.
fn read_table(path: &Path, table: &str, keys: &[&str], use_wal: bool) -> io::Result<HashMap<String, Vec<u8>>> {
    let mut out = HashMap::new();
    let Some(mut pager) = Pager::open(path, use_wal)? else { return Ok(out) };
    let root = pager
        .table_root(table)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no such table: {table}")))?;
    let mut wanted: HashSet<&[u8]> = keys.iter().map(|k| k.as_bytes()).collect();
    if wanted.is_empty() {
        return Ok(out);
    }
    pager.walk(root, &mut |pg, cell| {
        let cols = pg.record_header(cell)?;
        // Only text keys are compared; reading the key touches overflow pages only for very long keys.
        let Some(&(t, _)) = cols.first() else { return Ok(true) };
        if t < 13 || t % 2 == 0 {
            return Ok(true);
        }
        let Val::Text(key) = pg.column(cell, &cols, 0)? else { return Ok(true) };
        if !wanted.contains(key.as_slice()) {
            return Ok(true);
        }
        let value = match pg.column(cell, &cols, 1)? {
            Val::Null => None,
            Val::Int(v) => Some(v.to_string().into_bytes()),
            Val::Float(v) => Some(v.to_string().into_bytes()),
            Val::Text(b) | Val::Blob(b) => Some(b),
        };
        let key = String::from_utf8_lossy(&key).into_owned();
        wanted.remove(key.as_bytes());
        if let Some(v) = value {
            out.insert(key, v);
        }
        Ok(!wanted.is_empty())
    })?;
    Ok(out)
}

/// Reads `keys` from `ItemTable` of a VS Code style `state.vscdb` (including uncheckpointed WAL commits).
pub fn read_item_table(path: &Path, keys: &[&str]) -> io::Result<HashMap<String, Vec<u8>>> {
    read_table(path, "ItemTable", keys, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
    }

    /// A fresh scratch directory for tests that need modified copies of fixtures.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aqo-sqlite-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const TOKEN_KEY: &str = "cursorAuth/accessToken";
    const PLAN_KEY: &str = "cursorAuth/stripeMembershipType";

    fn value(i: usize) -> Vec<u8> {
        format!("value-{i}-").repeat(i % 7 + 1).into_bytes()
    }
    fn huge_blob() -> Vec<u8> {
        (0..20000u32).map(|j| ((j * 7 + 3) % 251) as u8).collect()
    }
    fn long_key() -> String {
        format!("long/{}", "k".repeat(3000))
    }

    #[test]
    fn varints() {
        assert_eq!(varint(&[0x05], 0), Some((5, 1)));
        assert_eq!(varint(&[0x81, 0x00], 0), Some((128, 2)));
        assert_eq!(varint(&[0xff; 9], 0), Some((u64::MAX, 9)));
        assert_eq!(varint(&[0x81], 0), None);
    }

    #[test]
    fn local_payload_rule() {
        // U = 4096: X = 4061, M = 489.
        let p = Pager {
            file: File::open(fixture("small.db")).unwrap(),
            file_len: 0,
            page_size: 4096,
            usable: 4096,
            page_count: 1,
            text_encoding: 1,
            wal: None,
        };
        assert_eq!(p.local_size(100), 100);
        assert_eq!(p.local_size(4061), 4061);
        // K = M + (P-M) % (U-4) is used only when it fits in X; otherwise M.
        assert_eq!(p.local_size(4062), 489);
        assert_eq!(p.local_size(489 + 4092 + 100), 589);
        assert_eq!(p.local_size(20000), 489 + (20000 - 489) % 4092);
        assert_eq!(p.local_size(489 + 4092 * 2 + 4000), 489);
    }

    #[test]
    fn small_db() {
        let m = read_item_table(&fixture("small.db"), &[TOKEN_KEY, PLAN_KEY, "number", "missing"]).unwrap();
        assert!(m[TOKEN_KEY].starts_with(b"eyJ"));
        assert_eq!(m[PLAN_KEY], b"pro"); // stored as a BLOB
        assert_eq!(m["number"], b"42");
        assert!(!m.contains_key("missing"));
        assert_eq!(m.len(), 3);
    }

    fn check_big(name: &str, page_size: usize) {
        let path = fixture(name);
        let mut pager = Pager::open(&path, true).unwrap().unwrap();
        assert_eq!(pager.page_size, page_size);
        // The fixture is meant to exercise interior pages in both sqlite_master and the table.
        assert_eq!(pager.page(1).unwrap()[100], 0x05, "sqlite_master should span interior pages");
        let root = pager.table_root("ItemTable").unwrap().unwrap();
        assert_eq!(pager.page(root).unwrap()[0], 0x05, "ItemTable should have interior pages");

        let lk = long_key();
        let names: Vec<String> = (0..2000).map(|i| format!("key/{i:04}")).collect();
        let mut keys: Vec<&str> = names.iter().map(String::as_str).collect();
        keys.extend(["huge/blob", "huge/text", lk.as_str()]);
        let m = read_item_table(&path, &keys).unwrap();
        for (i, k) in names.iter().enumerate() {
            if i % 13 == 0 {
                assert!(!m.contains_key(k), "{k} was deleted");
            } else {
                assert_eq!(m.get(k), Some(&value(i)), "{k}");
            }
        }
        assert_eq!(m["huge/blob"], huge_blob());
        assert_eq!(m["huge/text"], "abcdefghij".repeat(900).into_bytes());
        assert_eq!(m[&lk], b"long-key-value");

        // A lookup of only a few keys stops early and still finds them.
        let m = read_item_table(&path, &["key/0001", "huge/blob"]).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m["huge/blob"], huge_blob());
    }

    #[test]
    fn big_db_1024() {
        check_big("big-1024.db", 1024);
    }

    #[test]
    fn big_db_4096() {
        check_big("big-4096.db", 4096);
    }

    #[test]
    fn wal_db() {
        let path = fixture("wal.db");
        assert!(wal_path(&path).exists());
        let keys = [TOKEN_KEY, PLAN_KEY, "wal/only"];
        let m = read_item_table(&path, &keys).unwrap();
        assert_eq!(m[TOKEN_KEY], b"new-2");
        assert_eq!(m[PLAN_KEY], b"pro-wal");
        assert_eq!(m["wal/only"], b"only in wal");
        // The main file alone still has the checkpointed values.
        let old = read_table(&path, "ItemTable", &keys, false).unwrap();
        assert_eq!(old[TOKEN_KEY], b"old-token");
        assert_eq!(old[PLAN_KEY], b"free");
        assert!(!old.contains_key("wal/only"));
    }

    #[test]
    fn wal_trailing_garbage_and_bad_frames() {
        let dir = scratch("wal-garbage");
        let db = dir.join("wal.db");
        std::fs::copy(fixture("wal.db"), &db).unwrap();
        let mut wal = std::fs::read(fixture("wal.db-wal")).unwrap();
        wal.extend(std::iter::repeat_n(0xA5u8, 5000));
        std::fs::write(wal_path(&db), &wal).unwrap();
        let m = read_item_table(&db, &[TOKEN_KEY]).unwrap();
        assert_eq!(m[TOKEN_KEY], b"new-2");

        // A broken WAL header: the WAL is ignored and the main file's values are read.
        wal[24] ^= 0xff;
        std::fs::write(wal_path(&db), &wal).unwrap();
        let m = read_item_table(&db, &[TOKEN_KEY]).unwrap();
        assert_eq!(m[TOKEN_KEY], b"old-token");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_and_empty() {
        let dir = scratch("missing");
        let err = read_item_table(&dir.join("nope.db"), &[TOKEN_KEY]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        std::fs::write(dir.join("empty.db"), b"").unwrap();
        assert!(read_item_table(&dir.join("empty.db"), &[TOKEN_KEY]).unwrap().is_empty());
        std::fs::write(dir.join("junk.db"), vec![7u8; 4096]).unwrap();
        assert_eq!(read_item_table(&dir.join("junk.db"), &[TOKEN_KEY]).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let err = read_table(&fixture("small.db"), "NoSuchTable", &[TOKEN_KEY], true).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corruption_never_panics() {
        let dir = scratch("fuzz");
        let db = dir.join("x.db");
        let lk = long_key();
        for name in ["big-1024.db", "small.db"] {
            let orig = std::fs::read(fixture(name)).unwrap();
            // Truncations.
            for cut in [50, 100, 1000, 1024, 3000, orig.len() / 2, orig.len() - 1] {
                std::fs::write(&db, &orig[..cut.min(orig.len())]).unwrap();
                let _ = read_item_table(&db, &["key/0005", "huge/blob", &lk]);
            }
            // Random byte flips (deterministic LCG).
            let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
            for _ in 0..150 {
                let mut data = orig.clone();
                for _ in 0..8 {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let at = (seed >> 33) as usize % data.len();
                    data[at] ^= (seed >> 24) as u8 | 1;
                }
                std::fs::write(&db, &data).unwrap();
                let _ = read_item_table(&db, &["key/0005", "key/1999", "huge/blob", &lk]);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
