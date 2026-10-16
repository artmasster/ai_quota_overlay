// Renders the overlay with demo data and saves the README screenshots to docs/.
// Run: npx electron scripts/screenshots.js
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const OUT = path.join(__dirname, '..', 'docs');
const SRC = path.join(__dirname, '..', 'src');
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
    { id: 'five_hour', span: 5 * 3600, usedPercent: 38, resetsAt: now + 2.7 * H },
    { id: 'seven_day', span: 168 * 3600, usedPercent: 57, resetsAt: now + 4.4 * D },
    { id: 'seven_day_opus', span: 168 * 3600, name: 'Opus', usedPercent: 22, resetsAt: now + 4.4 * D },
  ] },
  codex: { ok: true, plan: 'pro', windows: [
    { id: 'primary', span: 5 * 3600, usedPercent: 12, resetsAt: now + 3.1 * H },
    { id: 'secondary', span: 168 * 3600, usedPercent: 71, resetsAt: now + 3.2 * D },
  ], credits: { balance: 1972.7 } },
  cursor: { ok: true, plan: 'pro', windows: [{ id: 'billing', billing: true, usedPercent: 86, resetsAt: now + 11.3 * D }], extra: null },
};
const seen = Object.fromEntries(['claude', 'codex', 'cursor'].map((k) =>
  [k, demo[k].windows.map(({ id, span, name, tag, billing }) => ({ id, span, name, tag, billing }))]));
const base = {
  lang: 'en', vertical: false, opacity: 0.92, network: false,
  services: { claude: true, codex: true, cursor: true },
  // The Opus and Codex 5 h windows are switched off, as one would in Settings.
  hidden: { claude: ['seven_day_opus'], codex: ['primary'], cursor: [] },
  seen, startup: true, canStartAtLogin: true, visible: true, version: require('../package.json').version,
};

const version = base.version;
const updateAvailable = { phase: 'available', current: version, version: '1.3.0', skipped: false, dismissed: false };

// name, settings override, open panel ({ svc, index } of a ring) or 'toast' (update toast), data override, network,
// update state
const SHOTS = [
  ['hero', {}, { svc: 'claude', index: 0 }],
  ['horizontal', {}, null],
  ['vertical', { vertical: true }, { svc: 'codex', index: 1 }],
  ['credits', {}, { svc: 'codex', index: 1 }],
  ['network', { network: true }, null, null, { online: true, ms: 23 }],
  ['thai', { lang: 'th' }, { svc: 'claude', index: 1 }],
  ['error', {}, { svc: 'claude', index: -1 }, { claude: { ok: false, error: { code: 'claudeExpired', status: 400 } } }],
  ['update', {}, 'toast', null, null, updateAvailable],
  ['update-progress', {}, 'toast', null, null, { ...updateAvailable, phase: 'downloading', percent: 64 }],
];

const hidden = (zoom, size = { width: 1200, height: 1400 }) => new BrowserWindow({
  ...size, show: false, transparent: true, frame: false, enableLargerThanScreen: true,
  webPreferences: { preload: path.join(__dirname, 'screenshot-preload.js'), zoomFactor: zoom },
});
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// One view of the app (card, panel or update toast) with demo data, cropped to its element.
async function capture(view, cfg, data, selected, network, update) {
  const win = hidden(ZOOM);
  await win.loadFile(path.join(SRC, view === 'toast' ? 'toast.html' : 'index.html'), { query: { view } });
  await win.webContents.insertCSS('.detail.show, .toast { animation: none !important; }');
  const send = (name, payload) => win.webContents.send('demo', name, payload);
  send('settings', cfg);
  if (network) send('network', network);
  if (update) send('update', update);
  send('quota', data);
  send('selected', selected);
  send('reveal');
  await wait(300);
  const selector = { panel: '.detail', toast: '.toast' }[view] ?? '.card';
  const r = await win.webContents.executeJavaScript(`(() => {
    const r = document.querySelector('${selector}').getBoundingClientRect();
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

// The whole Settings window, tall enough to show everything without scrolling.
async function captureSettings(cfg, network) {
  const zoom = 2;
  const width = 440;
  const win = hidden(zoom, { width: width * zoom, height: 1400 * zoom });
  win.setBackgroundColor('#111318');
  await win.loadFile(path.join(SRC, 'settings.html'));
  const send = (name, payload) => win.webContents.send('demo', name, payload);
  send('settings', cfg);
  if (network) send('network', network);
  send('quota', demo);
  send('update', updateAvailable);
  await wait(300);
  const h = await win.webContents.executeJavaScript(
    `document.querySelector('.titlebar').offsetHeight + document.querySelector('main').scrollHeight`);
  win.setSize(width * zoom, Math.ceil(h * zoom));
  await wait(200);
  const img = await win.webContents.capturePage({ x: 0, y: 0, width: width * zoom, height: Math.ceil(h * zoom) });
  win.destroy();
  return img;
}

app.on('window-all-closed', () => {});
app.whenReady().then(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  for (const [name, override, selected, dataOverride, network, update] of SHOTS) {
    const cfg = { ...base, ...override };
    const data = { ...demo, ...dataOverride };
    const toast = selected === 'toast';
    const card = await capture('card', cfg, data, toast ? null : selected, network);
    const panel = selected && await capture(toast ? 'toast' : 'panel', cfg, data, toast ? null : selected, network, update);
    const img = await compose(card, panel, cfg.vertical);
    fs.writeFileSync(path.join(OUT, `${name}.png`), img.toPNG());
    console.log('wrote', `docs/${name}.png`);
  }
  for (const lang of ['en', 'th']) {
    const img = await captureSettings({ ...base, lang, network: true }, { online: true, ms: 23 });
    fs.writeFileSync(path.join(OUT, `settings-${lang}.png`), img.toPNG());
    console.log('wrote', `docs/settings-${lang}.png`);
  }
  app.quit();
});
