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
- 📶 **สถานะอินเทอร์เน็ต** (ไม่บังคับ) — ปิดไว้เป็นค่าเริ่มต้น เปิดแล้วจะมีไฟเขียว + ping, หลุดเน็ตไฟแดงกะพริบ
- 🙈 **แสดงเฉพาะบริการที่ login ไว้** — ไม่ได้ใช้ Cursor ก็ไม่ขึ้น
- 🖱️ **ลากไปวางตรงไหนก็ได้** ทุกจอ (จอ scale ต่างกันก็ได้) · แนวนอน/แนวตั้ง · ปรับความทึบ
- 🎮 **ไม่บังงาน** — ลอยบนสุด, ไม่แย่ง focus, กินที่แค่ตัวกล่อง
- 🔒 **ไม่ต้องใส่รหัสอะไร** — ใช้ login ที่แอปของแต่ละเจ้ามีอยู่แล้วในเครื่อง

## 📥 ดาวน์โหลด

ไปที่ **[Releases ล่าสุด](https://github.com/artmasster/ai_quota_overlay/releases/latest)** แล้วเลือกไฟล์ตามเครื่อง

| ระบบ | ไฟล์ |
|---|---|
| 🪟 **Windows 10/11** | `AiQuotaOverlay-x.y.z-win-x64-setup.exe` |
| 🍎 **macOS** Apple Silicon (M1–M4) | `AiQuotaOverlay-x.y.z-mac-arm64.dmg` |
| 🍎 **macOS** Intel | `AiQuotaOverlay-x.y.z-mac-x64.dmg` |
| 🐧 **Linux** (ทุก distro) | `AiQuotaOverlay-x.y.z-linux-x86_64.AppImage` |
| 🐧 **Ubuntu / Debian** | `AiQuotaOverlay-x.y.z-linux-amd64.deb` |

<details>
<summary><b>เปิดครั้งแรกแล้วโดนเตือน? (แอปยังไม่ได้ code-sign)</b></summary>

- **Windows** — SmartScreen ขึ้น "Windows protected your PC" → กด **More info → Run anyway**
- **macOS** — ลากแอปไปไว้ใน Applications แล้วรันใน Terminal ครั้งเดียว:
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
| **คลิก `⋯`** ท้ายกล่อง | เปิดหน้าตั้งค่า |
| **ลาก** ตรงไหนของกล่องก็ได้ | ย้ายตำแหน่ง — จำไว้ให้ครั้งหน้า |
| **ดับเบิลคลิก** พื้นที่ว่างในกล่อง | รีเฟรชทันที (ปกติเช็คเองทุก 3 นาที) |
| **คลิกขวา** ที่กล่องหรือไอคอน tray | เปิดเมนู |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Q</kbd> | ซ่อน / แสดง |

**เมนูคลิกขวา**

| เมนู | ทำอะไร |
|---|---|
| **Settings…** | เปิดหน้าตั้งค่า (เหมือนกด `⋯`) |
| **Refresh** | เช็คโควต้าใหม่ตอนนี้ |
| **Rotate to vertical / horizontal** | สลับแนวนอน ↔ แนวตั้ง |
| **Snap to corner** | กลับไปมุมขวาล่างของจอหลัก |
| **Opacity** | ความทึบ 50 – 100% |
| **Launch at startup** | เปิดเองตอนเปิดเครื่อง (Windows / macOS) |
| **Hide / Show** | ซ่อน / แสดง |
| **Restart** | ปิดแล้วเปิดแอปใหม่ โดยเก็บการตั้งค่าเดิมไว้ |
| **Quit** | ปิดแอป |

กล่องรายละเอียดจะเปิดไปทาง **กลางจอ** เสมอ — วางไว้มุมขวาล่าง กล่องจะเปิดขึ้นบน, วางแนวตั้งชิดขวา กล่องจะเปิดไปทางซ้าย

## ⚙️ หน้าตั้งค่า

กด **`⋯`** ท้ายกล่อง (หรือคลิกขวา → **Settings…**) ทุกอย่างมีผลทันที ไม่ต้องรีสตาร์ท

| ส่วน | ตั้งอะไรได้ |
|---|---|
| **General** | ภาษา English / ไทย · แนวนอน / แนวตั้ง · ความทึบ 50 – 100% · เปิดพร้อมเครื่อง |
| **Internet status** | เปิด/ปิดไฟสถานะเน็ต + ping บนกล่อง (ค่าเริ่มต้น: **ปิด** — ปิดอยู่จะไม่ ping เลย) |
| **Services** | เปิด/ปิด Claude · Codex · Cursor ทีละเจ้า (ปิดแล้วไม่เช็คโควต้าเจ้านั้นเลย) และติ๊กเลือก **รอบโควต้าแต่ละอัน** ที่จะให้ขึ้นบนกล่อง พร้อม % ที่เหลือ |

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

### จากหน้าตั้งค่า / เมนู (ไม่ต้องแก้โค้ด)

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
| ขนาดทั้งหมด (ตอนนี้ 120%) | `SCALE` ใน [`src/main.js`](src/main.js) |
| ความถี่ในการเช็ค (ตอนนี้ 3 นาที) | `POLL_MS` ใน [`src/main.js`](src/main.js) |
| ระยะห่างจากขอบจอตอน Snap | `MARGIN` ใน [`src/main.js`](src/main.js) |
| คีย์ลัดซ่อน/แสดง | `globalShortcut.register('Control+Alt+Q', …)` ใน [`src/main.js`](src/main.js) |
| สีเขียว/เหลือง/แดง, พื้นหลัง, ตัวอักษร | ตัวแปร CSS ใน `:root` ของ [`src/index.html`](src/index.html) |
| เกณฑ์สี (50% / 20%) | ฟังก์ชัน `color()` ใน [`src/index.html`](src/index.html) |
| สีประจำบริการ | `SERVICES` ใน [`src/index.html`](src/index.html) |
| ข้อความ EN / TH, เพิ่มภาษาใหม่ | [`src/i18n.js`](src/i18n.js) — เพิ่มชุดข้อความใน `STRINGS` และใส่ใน `LANGS` |
| หน้าตั้งค่า | [`src/settings.html`](src/settings.html) |
| เพิ่มบริการใหม่ | [`src/quota.js`](src/quota.js) — เขียน `fetchXxx()` ที่คืน `{ plan, windows: [{ id, span, usedPercent, resetsAt }] }` (`span` = วินาที) แล้วใส่ใน `FETCHERS` |
| ไอคอน | `npm run icon` (วาดใหม่จาก [`scripts/make-icon.js`](scripts/make-icon.js)) |

## 🛠️ พัฒนา

ต้องมี **Node.js 22** (Node 24 แตกไฟล์ Electron ไม่ครบ — ดูด้านล่าง)

```bash
git clone git@github.com:artmasster/ai_quota_overlay.git
cd ai_quota_overlay
npm install
npm start
```

| คำสั่ง | ทำอะไร |
|---|---|
| `npm start` | รันจากซอร์ส (Windows ดับเบิลคลิก `start-overlay.vbs` ได้ ไม่มีหน้าต่าง console) |
| `npm run check` | พิมพ์ข้อมูลโควต้าที่แอปเห็น โดยไม่เปิดหน้าต่าง |
| `npm run dist` | build ตัวติดตั้งของเครื่องที่ใช้อยู่ลง `dist/` (`dist:win` · `dist:mac` · `dist:linux`) |
| `npx electron scripts/screenshots.js` | ถ่ายภาพตัวอย่างใน `docs/` ใหม่ด้วยข้อมูลจำลอง |
| `QUOTA_OVERLAY_SNAPSHOT=out.png npm start` | ถ่ายภาพ overlay จริงหลังโหลดเสร็จแล้วปิดตัว |

### ออก release

push tag ที่ขึ้นต้นด้วย `v` แล้ว GitHub Actions จะ build Windows / macOS / Linux และสร้าง Release ให้เอง

```bash
npm version patch   # หรือ minor / major — แก้ package.json + สร้าง tag
git push --follow-tags
```

อยาก build ทดสอบโดยไม่ออก release: แท็บ **Actions → Release → Run workflow** ไฟล์จะอยู่ใน artifacts ของรอบนั้น

<details>
<summary><b>Node 24: <code>npm install</code> แล้ว Electron เปิดไม่ขึ้น</b></summary>

ตัวแตกไฟล์ของ Electron พังเงียบๆ บน Node 24 (`node_modules/electron/dist` มีแค่ `locales/sr.pak`) ใช้ Node 22 หรือแตกเองจาก cache:

```powershell
Remove-Item node_modules\electron\dist -Recurse -Force
Expand-Archive (Get-ChildItem $env:LOCALAPPDATA\electron\Cache -Recurse -Filter 'electron-*-win32-x64.zip')[0].FullName node_modules\electron\dist
Set-Content node_modules\electron\path.txt 'electron.exe' -NoNewline
```
</details>

## ❓ แก้ปัญหา

| อาการ | ทำไง |
|---|---|
| วงขึ้น `!` | คลิกที่วงเพื่ออ่านสาเหตุ — ส่วนใหญ่คือ login หมดอายุ เปิดแอปของบริการนั้นสักครั้ง (Claude: `claude` แล้ว `/login`) |
| บริการที่ใช้อยู่ไม่ขึ้นเลย | ยังไม่ได้ login ในเครื่องนี้ หรือ login ด้วยวิธีอื่น (เช่น Codex ใช้ API key แทนบัญชี ChatGPT) |
| ไม่ขึ้นทับเกม | ใช้โหมด **Borderless / Windowed** — Exclusive fullscreen ไม่มี overlay ไหนขึ้นได้ |
| Linux: พื้นหลังดำ | หน้าต่างโปร่งใสต้องใช้ desktop ที่มี compositor |

## 📄 License

[MIT](LICENSE) © artmasster
