"""Writes an electron-builder style update feed (latest.yml / latest-linux.yml) for release files.

    python -I scripts/latest-yml.py <version> <out.yml> <file> [file ...]

The first file is the one installed apps download (`path`); every file is listed under `files` with its size and
base64 SHA-512, which is what electron-updater (Electron builds up to 1.2.0) and the Rust updater check.
Run it on the final, signed files.
"""
import base64
import datetime
import hashlib
import sys
from pathlib import Path

version, out, *files = sys.argv[1:]
if not files:
    sys.exit(__doc__)


def sha512(p: Path) -> str:
    h = hashlib.sha512()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return base64.b64encode(h.digest()).decode()


entries = [(Path(f).name, sha512(Path(f)), Path(f).stat().st_size) for f in files]
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.") + "000Z"
lines = [f"version: {version}", "files:"]
for name, digest, size in entries:
    lines += [f"  - url: {name}", f"    sha512: {digest}", f"    size: {size}"]
lines += [f"path: {entries[0][0]}", f"sha512: {entries[0][1]}", f"releaseDate: '{now}'", ""]
Path(out).write_text("\n".join(lines), encoding="utf-8", newline="\n")
print(Path(out).read_text(encoding="utf-8"))
