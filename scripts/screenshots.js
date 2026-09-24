// Renders the overlay with demo data and saves the README screenshots to docs/.
// Run: npx electron scripts/screenshots.js
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const OUT = path.join(__dirname, '..', 'docs');
const ZOOM = 1.2 * 2; // app scale × 2 for crisp images on high-DPI screens

const now = Date.now();
const H = 3600e3;
const D = 24 * H;
const demo = {
  fetchedAt: now,
  claude: { ok: true, plan: 'max', windows: [
    { label: '5 ชม.', usedPercent: 38, resetsAt: now + 2.7 * H },
    { label: '7 วัน', usedPercent: 57, resetsAt: now + 4.4 * D },
  ] },
  codex: { ok: true, plan: 'pro', windows: [{ label: '7 วัน', usedPercent: 71, resetsAt: now + 3.2 * D }], credits: { balance: 1972.7 } },
  cursor: { ok: true, plan: 'pro', windows: [{ label: 'รอบบิล', usedPercent: 86, resetsAt: now + 11.3 * D }], extra: null },
};

// name, css viewport size, layout, anchor, ring to click (index into .ring), data override
const SHOTS = [
  ['hero', [330, 300], { vertical: false }, { right: true, bottom: true }, 0],
  ['horizontal', [260, 116], { vertical: false }, { right: true, bottom: true }, null],
  ['vertical', [372, 380], { vertical: true }, { right: true, bottom: true }, 2],
  ['credits', [330, 300], { vertical: false }, { right: true, bottom: true }, 2],
  ['error', [330, 260], { vertical: false }, { right: true, bottom: true }, 0,
    { claude: { ok: false, error: 'login หมดอายุ (400) — เปิด terminal พิมพ์ claude แล้ว /login' } }],
];

const EXTRA_CSS = '.detail.show { animation: none !important; }';
const MARGIN = 6; // css px of transparent space kept around the cropped content

app.on('window-all-closed', () => {});
app.whenReady().then(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  for (const [name, [w, h], layout, anchor, click, override] of SHOTS) {
    const win = new BrowserWindow({
      width: Math.round(w * ZOOM), height: Math.round(h * ZOOM), show: false,
      transparent: true, frame: false, backgroundColor: '#00000000',
      webPreferences: { preload: path.join(__dirname, 'screenshot-preload.js') },
    });
    await win.loadFile(path.join(__dirname, '..', 'src', 'index.html'));
    win.webContents.setZoomFactor(ZOOM);
    await win.webContents.insertCSS(EXTRA_CSS);
    const send = (n, p) => win.webContents.send('demo', n, p);
    send('anchor', anchor);
    send('layout', layout);
    send('quota', { ...demo, ...override });
    await new Promise((r) => setTimeout(r, 300));
    if (click !== null) {
      await win.webContents.executeJavaScript(`(() => {
        const r = document.querySelectorAll('.ring')[${click}];
        r.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 1 }));
        document.getElementById('card').dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 1 }));
      })()`);
    }
    await new Promise((r) => setTimeout(r, 300));
    // Crop to the card plus the open panel (if any), with a small transparent margin.
    const box = await win.webContents.executeJavaScript(`(() => {
      const rs = [...document.querySelectorAll('.card, .detail.show')].map((e) => e.getBoundingClientRect());
      const x = Math.min(...rs.map((r) => r.left)), y = Math.min(...rs.map((r) => r.top));
      return { x, y, w: Math.max(...rs.map((r) => r.right)) - x, h: Math.max(...rs.map((r) => r.bottom)) - y };
    })()`);
    const img = await win.webContents.capturePage({
      x: Math.max(0, Math.floor((box.x - MARGIN) * ZOOM)),
      y: Math.max(0, Math.floor((box.y - MARGIN) * ZOOM)),
      width: Math.ceil((box.w + 2 * MARGIN) * ZOOM),
      height: Math.ceil((box.h + 2 * MARGIN) * ZOOM),
    });
    fs.writeFileSync(path.join(OUT, `${name}.png`), img.toPNG());
    console.log('wrote', `docs/${name}.png`);
    win.destroy();
  }
  app.quit();
});
