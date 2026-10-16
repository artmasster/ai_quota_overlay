<div align="center">

<img src="build/icon.png" width="96" alt="Ai Quota Overlay icon">

# Ai Quota Overlay

**วงล้อเล็กๆ ลอยบนจอ บอกว่าโควต้า Claude · Codex · Cursor เหลือเท่าไหร่ และจะรีเซ็ตเมื่อไหร่**

*Tiny always-on-top rings showing how much of your Claude, Codex and Cursor quota is left.*

[![Release](https://img.shields.io/github/v/release/artmasster/ai_quota_overlay?style=flat-square&color=10a37f)](https://github.com/artmasster/ai_quota_overlay/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/artmasster/ai_quota_overlay/total?style=flat-square&color=d97757)](https://github.com/artmasster/ai_quota_overlay/releases)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-a5b4fc?style=flat-square)
[![License](https://img.shields.io/github/license/artmasster/ai_quota_overlay?style=flat-square)](LICENSE)

<img src="docs/hero.png" width="380" alt="Overlay with the Claude detail panel open">

</div>

---

## ✨ ทำอะไรได้

- 🟢 **วงล้อบอก % ที่เหลือ** — เขียว > 50% · เหลือง > 20% · แดง ≤ 20%
- ⏳ **ตัวเลขใต้วง = เวลาก่อนรีเซ็ต** — `2h` · `4.3d` · `35m` นับลงเอง
- 👆 **คลิกที่วง** เปิดกล่องรายละเอียด: เหลือ/ใช้ไป, รีเซ็ตวันไหนกี่โมง, เครดิต
- ⚙️ **หน้าตั้งค่าแยก** — กด `⋯` ท้ายกล่อง เลือกได้ว่าจะให้ **บริการไหน** และ **รอบโควต้าไหน** (5 ชม. / 7 วัน / Opus …) ขึ้นบนกล่อง
- 🌐 **2 ภาษา** — English (ค่าเริ่มต้น) / ไทย สลับได้ในหน้าตั้งค่า
- ⬆️ **อัปเดตในแอป** — มีเวอร์ชันใหม่ขึ้นเตือนเล็กๆ กด Update แล้วดาวน์โหลด ติดตั้ง รีสตาร์ทให้เอง หรือกดข้ามเวอร์ชันนั้นได้
- 📶 **สถานะอินเทอร์เน็ต** (ไม่บังคับ) — ปิดไว้เป็นค่าเริ่มต้น เปิดแล้วจะมีไฟเขียว + ping, หลุดเน็ตไฟแดงกะพริบ
- 🙈 **แสดงเฉพาะบริการที่ login ไว้** — ไม่ได้ใช้ Cursor ก็ไม่ขึ้น
- 🖱️ **ลากไปวางตรงไหนก็ได้** ทุกจอ (จอ scale ต่างกันก็ได้) · แนวนอน/แนวตั้ง · ปรับความทึบ
- ⚡ **อัปเดตไว แต่ไม่รัว** — โควต้าทุก 1 นาที (ซ่อนอยู่ทุก 5 นาที), ไม่ยิงซ้อนกัน, โดน 429 จะพักเจ้านั้น 2 → 15 นาทีแล้วโชว์ตัวเลขล่าสุดไว้ก่อน
- 🎮 **ไม่บังงาน** — ลอยบนสุด, ไม่แย่ง focus, กินที่แค่ตัวกล่อง
- 🔒 **ไม่ต้องใส่รหัสอะไร** — ใช้ login ที่แอปของแต่ละเจ้ามีอยู่แล้วในเครื่อง
- 🪶 **เบามาก** — เขียนด้วย Rust วาดเองทั้งหมด ไม่มี browser/webview ข้างใน: ตัวติดตั้ง Windows ~1.4 MB (เดิม 94 MB), RAM ~25 MB, CPU แทบ 0% ตอนว่าง

## 📥 ดาวน์โหลด

ไปที่ **[Releases ล่าสุด](https://github.com/artmasster/ai_quota_overlay/releases/latest)** แล้วเลือกไฟล์ตามเครื่อง

| ระบบ | ไฟล์ |
|---|---|
| 🪟 **Windows 10/11** | `AiQuotaOverlay-x.y.z-win-x64-setup.exe` |
| 🍎 **macOS** Apple Silicon (M1–M4) | `AiQuotaOverlay-x.y.z-mac-arm64.dmg` |
| 🍎 **macOS** Intel | `AiQuotaOverlay-x.y.z-mac-x64.dmg` |
| 🐧 **Linux** (ทุก distro) | `AiQuotaOverlay-x.y.z-linux-x86_64.AppImage` |
| 🐧 **Ubuntu / Debian** | `AiQuotaOverlay-x.y.z-linux-amd64.deb` |

ตัว Windows sign ด้วยใบรับรอง code signing แล้ว ใช้ Electron เวอร์ชันเก่า (1.2.0) อยู่ แอปจะเตือนอัปเดตให้เอง กด Update ได้เลย ระบบจะเปลี่ยนเป็นตัวใหม่ในโฟลเดอร์เดิม และ settings เดิมยังอยู่ครบ

<details>
<summary><b>macOS / Linux เปิดครั้งแรกแล้วโดนเตือน?</b></summary>

- **macOS** (ยังไม่ได้ sign) — ลากแอปไปไว้ใน Applications แล้วรันใน Terminal ครั้งเดียว:
  ```bash
  xattr -cr "/Applications/Ai Quota Overlay.app"
  ```
  หรือคลิกขวาที่แอป → **Open** → **Open**
- **Linux AppImage** — ให้สิทธิ์รันก่อน:
  ```bash
  chmod +x AiQuotaOverlay-*.AppImage && ./AiQuotaOverlay-*.AppImage
  ```
</details>

## 🔑 ต้อง login อะไรไว้บ้าง

แอปอ่าน login ที่โปรแกรมของแต่ละเจ้าเก็บไว้ในเครื่องอยู่แล้ว **login ไว้ตัวไหน ก็ขึ้นตัวนั้น**

| บริการ | ต้องมี | login เก็บที่ |
|---|---|---|
| <img src="https://img.shields.io/badge/-Claude-d97757?style=flat-square"> | [Claude Code](https://claude.com/claude-code) CLI — พิมพ์ `claude` แล้ว `/login` | Windows/Linux: `~/.claude/.credentials.json` · macOS: Keychain |
| <img src="https://img.shields.io/badge/-Codex-10a37f?style=flat-square"> | [Codex](https://openai.com/codex) (แอปหรือ CLI) login ด้วยบัญชี ChatGPT | `~/.codex/auth.json` |
| <img src="https://img.shields.io/badge/-Cursor-a5b4fc?style=flat-square"> | แอป [Cursor](https://cursor.com) ที่ login แล้ว | `state.vscdb` ในโฟลเดอร์ config ของ Cursor |

> [!NOTE]
> token ถูกส่งไปแค่เซิร์ฟเวอร์ของเจ้าของบริการเอง (`api.anthropic.com` · `chatgpt.com` · `cursor.com`) ไม่มีเซิร์ฟเวอร์กลาง ไม่มีการเก็บข้อมูล
> ตัวเลขโควต้ามาจาก API ภายในที่ไม่ได้เปิดเป็นทางการ ถ้าวันหนึ่งเจ้าของเปลี่ยนรูปแบบ วงจะขึ้น `!` แทนตัวเลขจนกว่าจะอัปเดตแอป

## 🖱️ วิธีใช้

| ทำอะไร | ผล |
|---|---|
| **คลิกที่วง** | เปิดกล่องรายละเอียด (คลิกด้านนอก / คลิกวงเดิม / `×` เพื่อปิด) |
| **คลิก `⋯`** ท้ายกล่อง หรือ **คลิกขวา** ที่กล่อง | เปิดหน้าแอป (ตั้งค่า) |
| **คลิกซ้าย / ขวา** ที่ไอคอน tray (มุมขวาล่าง ข้างนาฬิกา) | เปิดหน้าแอป |
| **ลาก** ตรงไหนของกล่องก็ได้ | ย้ายตำแหน่ง — จำไว้ให้ครั้งหน้า |
| **ดับเบิลคลิก** พื้นที่ว่างในกล่อง | รีเฟรชทันที (ปกติเช็คเองทุก 1 นาที) |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Q</kbd> | ซ่อน / แสดงกล่อง |

กล่องรายละเอียดจะเปิดไปทาง **กลางจอ** เสมอ — วางไว้มุมขวาล่าง กล่องจะเปิดขึ้นบน, วางแนวตั้งชิดขวา กล่องจะเปิดไปทางซ้าย

## ⚙️ หน้าแอป

ไม่มีเมนูคลิกขวาแล้ว ทุกอย่างย้ายมาอยู่ในหน้าแอปหน้าเดียว เปิดได้จาก `⋯` / คลิกขวาที่กล่อง / ไอคอน tray / เปิดแอปซ้ำจาก Start menu ทุกอย่างมีผลทันที ไม่ต้องรีสตาร์ท
(Linux: tray ไม่มี event คลิก จึงมีเมนูเล็กๆ ไว้เปิดหน้าแอปหรือปิดแอป)

| ส่วน | ตั้งอะไรได้ |
|---|---|
| **General** | แสดง / ซ่อนกล่อง · ภาษา English / ไทย · แนวนอน / แนวตั้ง · ความทึบ 50 – 100% · เปิดพร้อมเครื่อง |
| **Internet status** | เปิด/ปิดไฟสถานะเน็ต + ping ทุก 3 วินาที (ค่าเริ่มต้น: **ปิด** — ปิดอยู่หรือซ่อนกล่องไว้จะไม่ ping เลย) |
| **Services** | เปิด/ปิด Claude · Codex · Cursor ทีละเจ้า (ปิดแล้วไม่เช็คโควต้าเจ้านั้นเลย) และติ๊กเลือก **รอบโควต้าแต่ละอัน** ที่จะให้ขึ้นบนกล่อง พร้อม % ที่เหลือ |
| **Updates** | เวอร์ชันปัจจุบัน · เช็คอัปเดตตอนนี้ · อัปเดต (แม้จะเคยกดข้ามไว้) |
| **ปุ่มด้านล่าง** | Refresh now · Snap to corner (กลับมุมขวาล่าง) · Restart · Quit |

## ⬆️ อัปเดตแอป

แอปเช็ค [Releases](https://github.com/artmasster/ai_quota_overlay/releases) เองหลังเปิด 5 วินาที, ทุก 15 นาที, ตอนเปิดหน้าแอป และตอนเครื่องตื่นจาก sleep ถ้ามีเวอร์ชันใหม่จะขึ้นกล่องเล็กๆ ตรงที่กล่องรายละเอียดเปิด

- **Update** → ดาวน์โหลด (มีแถบ %) → ติดตั้งเงียบๆ → เปิดแอปใหม่ให้เอง ไม่ต้องกดอะไรต่อ
- **Skip this version** → ไม่เตือนเวอร์ชันนี้อีก จนกว่าจะมีเวอร์ชันที่ใหม่กว่า (ยังกดอัปเดตจากหน้าแอปได้)
- **×** → ปิดไว้ก่อน เตือนใหม่ตอนเปิดแอปครั้งหน้า

ไม่มีอะไรดาวน์โหลดเองโดยไม่ได้กด · macOS (แอปไม่ได้ sign) ปุ่ม Update จะเปิดหน้าดาวน์โหลดแทน

<table>
<tr>
<td align="center"><b>มีอัปเดต</b><br><img src="docs/update.png" width="300"></td>
<td align="center"><b>กำลังดาวน์โหลด</b><br><img src="docs/update-progress.png" width="300"></td>
</tr>
</table>

<table>
<tr>
<td align="center"><b>English</b><br><img src="docs/settings-en.png" width="330"></td>
<td align="center"><b>ไทย</b><br><img src="docs/settings-th.png" width="330"></td>
</tr>
</table>

## 🖼️ ตัวอย่าง

<table>
<tr>
<td align="center"><b>แนวนอน</b><br><img src="docs/horizontal.png" width="300"></td>
<td align="center" rowspan="2"><b>แนวตั้ง</b><br><img src="docs/vertical.png" width="340"></td>
</tr>
<tr>
<td align="center"><b>รายละเอียด + เครดิต</b><br><img src="docs/credits.png" width="300"></td>
</tr>
<tr>
<td align="center"><b>เปิดสถานะอินเทอร์เน็ต</b><br><img src="docs/network.png" width="300"></td>
<td align="center"><b>ภาษาไทย</b><br><img src="docs/thai.png" width="300"></td>
</tr>
<tr>
<td align="center" colspan="2"><b>login หมดอายุ — บอกวิธีแก้ในกล่อง</b><br><img src="docs/error.png" width="300"></td>
</tr>
</table>

## 🎨 ปรับแต่ง

### จากหน้าแอป (ไม่ต้องแก้โค้ด)

ทุกอย่างในหน้าตั้งค่า และตำแหน่งกล่อง ถูกเก็บไว้ที่ `settings.json`

| ระบบ | ที่อยู่ |
|---|---|
| Windows | `%APPDATA%\Ai Quota Overlay\settings.json` |
| macOS | `~/Library/Application Support/Ai Quota Overlay/settings.json` |
| Linux | `~/.config/Ai Quota Overlay/settings.json` |

```jsonc
{
  "card": { "x": 2360, "y": 1290, "w": 273, "h": 96 }, // มุมซ้ายบนและขนาดของกล่อง (null = มุมขวาล่าง)
  "opacity": 0.92,                                     // 0.5 – 1
  "vertical": false,                                   // true = แนวตั้ง
  "lang": "en",                                        // "en" | "th"
  "network": false,                                    // true = แสดงสถานะอินเทอร์เน็ต
  "services": { "claude": true, "codex": true, "cursor": false }, // false = ไม่เช็คเจ้านั้น
  "hidden": { "claude": ["seven_day_opus"] }           // id ของรอบโควต้าที่ไม่ให้ขึ้นบนกล่อง
}
```

### จำกัดบริการด้วย environment variable

ตั้ง `QUOTA_OVERLAY_SERVICES` ก่อนเปิดแอป (ค่าเริ่มต้น: ทุกตัวที่ login) — ใช้ร่วมกับสวิตช์ในหน้าตั้งค่าได้

```bash
QUOTA_OVERLAY_SERVICES=claude,codex
```

### แก้จากซอร์ส

| อยากเปลี่ยน | แก้ที่ |
|---|---|
| ขนาดทั้งหมด (ตอนนี้ 120%) | `SCALE` ใน [`src/app.rs`](src/app.rs) |
| ความถี่ในการเช็คโควต้า (ตอนนี้ 1 นาที, ซ่อนอยู่ 5 นาที) / เน็ต (3 วินาที) | `POLL` · `IDLE_POLL` · `NET_EVERY` ใน [`src/app.rs`](src/app.rs) |
| ความถี่ในการเช็คอัปเดต (ตอนนี้ 15 นาที) | `CHECK_EVERY` ใน [`src/updater.rs`](src/updater.rs) |
| ระยะห่างจากขอบจอตอน Snap | `MARGIN` ใน [`src/app.rs`](src/app.rs) |
| คีย์ลัดซ่อน/แสดง | `RegisterHotKey` ใน [`src/platform/windows.rs`](src/platform/windows.rs) · `HotKey::new` ใน [`src/platform/unix.rs`](src/platform/unix.rs) |
| สีเขียว/เหลือง/แดง, พื้นหลัง, ตัวอักษร | ค่าคงที่สีใน [`src/ui/mod.rs`](src/ui/mod.rs) |
| เกณฑ์สี (50% / 20%) | `level_color()` ใน [`src/ui/mod.rs`](src/ui/mod.rs) |
| สีประจำบริการ | `svc_color()` ใน [`src/ui/mod.rs`](src/ui/mod.rs) |
| ข้อความ EN / TH | [`src/i18n.rs`](src/i18n.rs) |
| กล่อง / กล่องรายละเอียด / toast อัปเดต / หน้าแอป | [`src/ui/card.rs`](src/ui/card.rs) · [`panel.rs`](src/ui/panel.rs) · [`toast.rs`](src/ui/toast.rs) · [`app_window.rs`](src/ui/app_window.rs) |
| เพิ่มบริการใหม่ | [`src/quota/`](src/quota) — เขียน fetcher ที่คืน `Quota { plan, windows: [Win { id, span, used_percent, resets_at }] }` (`span` = วินาที) แล้วใส่ใน `fetch_all` |

## 🛠️ พัฒนา

ต้องมี [Rust](https://rustup.rs) (stable) — แค่นั้น ไม่มี Node / Electron แล้ว

```bash
git clone git@github.com:artmasster/ai_quota_overlay.git
cd ai_quota_overlay
cargo run
```

| ระบบ | ต้องมีเพิ่ม |
|---|---|
| Windows | toolchain MSVC (Visual Studio Build Tools) **หรือ** `stable-x86_64-pc-windows-gnu` + [WinLibs](https://winlibs.com) (`winget install BrechtSanders.WinLibs.POSIX.MSVCRT`) ใน `PATH` |
| macOS | Xcode Command Line Tools |
| Linux | `build-essential` (X11 libs โหลดตอนรัน: libx11, libxcursor, libxrandr, libxi, libxkbcommon-x11) |

| คำสั่ง | ทำอะไร |
|---|---|
| `cargo run` | รันจากซอร์ส |
| `cargo test` | เทสต์ (ตัวดึงข้อมูล, ตัวอ่าน SQLite, settings, updater) — ไม่ใช้เน็ต ไม่ใช้ login จริง |
| `cargo run -- --screenshots docs` | ถ่ายภาพตัวอย่างใน `docs/` ใหม่ด้วยข้อมูลจำลอง (ทุกสถานะ EN/TH) |
| `python -I scripts/compare.py <เก่า> <ใหม่> <ผล>` | วางภาพเทียบ เก่า \| ใหม่ \| ส่วนต่าง |
| `QUOTA_OVERLAY_SNAPSHOT=out.png cargo run` | ถ่ายภาพ overlay จริงหลังโหลดเสร็จแล้วปิดตัว |
| `QUOTA_OVERLAY_FAKE_UPDATE=9.9.0 cargo run` | จำลองว่ามี v9.9.0 ออก — ลองกล่องอัปเดต / ข้าม / แถบดาวน์โหลด ได้ครบโดยไม่ติดตั้งจริง |
| `QUOTA_OVERLAY_DATA_DIR=<โฟลเดอร์>` | เก็บ settings ที่อื่น (ไม่ทับของจริง) |
| `QUOTA_OVERLAY_LOG_REQUESTS=<ไฟล์>` | บันทึกเวลา + URL ของทุก request (ไม่มี header/token) ไว้ตรวจจังหวะการเช็ค |

### ออก release

1. แก้ `version` ใน `Cargo.toml` แล้ว push tag `v<version>` — GitHub Actions จะ build ทั้ง 3 ระบบ และสร้าง Release แบบ **draft** ที่มีไฟล์ macOS / Linux + `latest-linux.yml`
2. ตัว Windows sign บนเครื่อง Siam (`ssh siam`, ใบรับรอง SSL.com eSigner): รัน `pwsh scripts/release-windows.ps1 -Publish` — build exe, sign exe / uninstaller / installer, สร้าง `latest.yml` จากไฟล์ที่ sign แล้ว, อัปโหลดเข้า draft แล้ว publish (ต้อง login `gh` ไว้)

แอปที่ติดตั้งอยู่อ่าน `latest.yml` (Windows) / `latest-linux.yml` (AppImage) แบบเดียวกับ electron-builder ตัว Electron เก่าจึงอัปเดตข้ามมาเป็นตัว Rust ได้เอง
อยาก build ทดสอบโดยไม่ออก release: แท็บ **Actions → Release → Run workflow** ไฟล์จะอยู่ใน artifacts ของรอบนั้น

ตัว Electron เดิมเก็บไว้ที่ branch [`old-electron`](https://github.com/artmasster/ai_quota_overlay/tree/old-electron)

## ❓ แก้ปัญหา

| อาการ | ทำไง |
|---|---|
| วงขึ้น `!` | คลิกที่วงเพื่ออ่านสาเหตุ — ส่วนใหญ่คือ login หมดอายุ เปิดแอปของบริการนั้นสักครั้ง (Claude: `claude` แล้ว `/login`) |
| บริการที่ใช้อยู่ไม่ขึ้นเลย | ยังไม่ได้ login ในเครื่องนี้ หรือ login ด้วยวิธีอื่น (เช่น Codex ใช้ API key แทนบัญชี ChatGPT) |
| ไม่ขึ้นทับเกม | ใช้โหมด **Borderless / Windowed** — Exclusive fullscreen ไม่มี overlay ไหนขึ้นได้ |
| Linux: พื้นหลังดำ | หน้าต่างโปร่งใสต้องใช้ desktop ที่มี compositor |
| Linux Wayland | แอปเปิดผ่าน XWayland (overlay ต้องกำหนดตำแหน่งเองได้ ซึ่ง Wayland ไม่ให้) — ต้องมี XWayland ในเครื่อง |

## 📄 License

[MIT](LICENSE) © artmasster
