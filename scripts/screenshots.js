// Renders the overlay with demo data and saves the README screenshots to docs/.
// Run: npx electron scripts/screenshots.js
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const OUT = path.join(__dirname, '..', 'docs');
const ZOOM = 1.2 * 2; // app scale × 2 for crisp images on high-DPI screens
const PAD = 2;        // css px around each element, so the panel's outline isn't cut off
const GAP = 4;        // css px between the card and the panel, as in the app
const MARGIN = 6;     // css px of transparent space around each image

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

// name, vertical, open panel ({ svc, index } of a ring), data override
const SHOTS = [
  ['hero', false, { svc: 'claude', index: 0 }],
  ['horizontal', false, null],
  ['vertical', true, { svc: 'codex', index: 0 }],
  ['credits', false, { svc: 'codex', index: 0 }],
  ['error', false, { svc: 'claude', index: -1 },
    { claude: { ok: false, error: 'login หมดอายุ (400) — เปิด terminal พิมพ์ claude แล้ว /login' } }],
];

const hidden = (zoom) => new BrowserWindow({
  width: 1200, height: 900, show: false, transparent: true, frame: false,
  webPreferences: { preload: path.join(__dirname, 'screenshot-preload.js'), zoomFactor: zoom },
});
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// One view of the app page (card or panel) with demo data, cropped to its element.
async function capture(view, vertical, data, selected) {
  const win = hidden(ZOOM);
  await win.loadFile(path.join(__dirname, '..', 'src', 'index.html'), { query: { view } });
  await win.webContents.insertCSS('.detail.show { animation: none !important; }');
  const send = (name, payload) => win.webContents.send('demo', name, payload);
  send('vertical', vertical);
  send('quota', data);
  send('selected', selected);
  send('reveal');
  await wait(300);
  const r = await win.webContents.executeJavaScript(`(() => {
    const r = document.querySelector('${view === 'panel' ? '.detail' : '.card'}').getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  })()`);
  const img = await win.webContents.capturePage({
    x: Math.floor((r.x - PAD) * ZOOM),
    y: Math.floor((r.y - PAD) * ZOOM),
    width: Math.ceil((r.w + 2 * PAD) * ZOOM),
    height: Math.ceil((r.h + 2 * PAD) * ZOOM),
  });
  win.destroy();
  return img.toDataURL();
}

// Puts the card and its panel together the way the app places them in the bottom-right corner of a screen:
// the panel above the card (horizontal) or to its left (vertical), sharing the edge that faces the corner.
async function compose(card, panel, vertical) {
  const html = `<body style="margin:0"><div id="shot" style="display:inline-flex; align-items:flex-end;
    flex-direction:${vertical ? 'row-reverse' : 'column-reverse'}; gap:${(GAP - 2 * PAD) * ZOOM}px;
    padding:${(MARGIN - PAD) * ZOOM}px">
    ${[card, panel].filter(Boolean).map((src) => `<img src="${src}" style="display:block">`).join('')}</div></body>`;
  const win = hidden(1);
  await win.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`);
  const r = await win.webContents.executeJavaScript(`(() => {
    const r = document.getElementById('shot').getBoundingClientRect();
    return { width: Math.ceil(r.width), height: Math.ceil(r.height) };
  })()`);
  const img = await win.webContents.capturePage({ x: 0, y: 0, ...r });
  win.destroy();
  return img;
}

app.on('window-all-closed', () => {});
app.whenReady().then(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  for (const [name, vertical, selected, override] of SHOTS) {
    const data = { ...demo, ...override };
    const card = await capture('card', vertical, data, selected);
    const panel = selected && await capture('panel', vertical, data, selected);
    const img = await compose(card, panel, vertical);
    fs.writeFileSync(path.join(OUT, `${name}.png`), img.toPNG());
    console.log('wrote', `docs/${name}.png`);
  }
  app.quit();
});
