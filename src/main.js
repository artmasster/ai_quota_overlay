const { app, BrowserWindow, Tray, Menu, nativeImage, globalShortcut, ipcMain, screen, net } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { fetchAll } = require('./quota');

const POLL_MS = 3 * 60 * 1000;
// Internet check: online when any of these answers within NET_TIMEOUT_MS. Any HTTP status counts, only a failed
// connection doesn't, so a captive portal or a blocked host still reads as online as long as one gets through.
const NET_MS = 5 * 1000;
const NET_TIMEOUT_MS = 4 * 1000;
const NET_URLS = ['https://www.gstatic.com/generate_204', 'https://1.1.1.1/cdn-cgi/trace'];
const MARGIN = 16;
// Whole UI is drawn at this zoom. Sizes from the page are CSS px, multiplied by SCALE for the windows.
const SCALE = 1.2;
// The card and the panel sit PAD inside their windows (room for the panel's outline) and GAP apart on screen.
const PAD = 2 * SCALE;
const GAP = 4 * SCALE;
// Run from source (`electron .`), a login item must also pass the app folder or it starts bare Electron.
const LOGIN_ITEM = process.defaultApp ? { args: [app.getAppPath()] } : {};

const settingsFile = () => path.join(app.getPath('userData'), 'settings.json');
// card = where the card was left and its size then; null means "bottom-right of the primary display".
let settings = { card: null, opacity: 0.92, vertical: false };
// The card and its detail panel get a window each, just big enough to hold them. A window takes the scale of the
// display holding most of it, so a small one rescales as soon as the card is dragged onto another monitor, and
// opening the panel never moves the card.
let cardWin;
let panelWin;
let tray;
let timer;
let netTimer;
let last = null;
let netState = null;                   // { online, ms } from the last internet check
let card = null;                       // { x, y } on screen
let cardSize = { w: 192, h: 91 };
let panelSize = { w: 0, h: 0 };
let selected = null;                   // { svc, index } of the ring whose panel is open
let focusPanelOnReveal = false;
let anchor = { right: true, bottom: true };
let dragFrom = null;

function loadSettings() {
  try { settings = { ...settings, ...JSON.parse(fs.readFileSync(settingsFile(), 'utf8')) }; } catch {}
}
function saveSettings() {
  try { fs.writeFileSync(settingsFile(), JSON.stringify(settings)); } catch {}
}

const trayIcon = () => nativeImage.createFromPath(path.join(__dirname, 'icon.png')).resize({ width: 16, height: 16 });
const windows = () => [cardWin, panelWin];

function defaultCard() {
  const area = screen.getPrimaryDisplay().workArea;
  return { x: area.x + area.width - cardSize.w - MARGIN, y: area.y + area.height - cardSize.h - MARGIN };
}

// Back where it was left, at its size then, so the corner it's anchored to stays put once the real size comes in.
function initialCard() {
  const saved = settings.card;
  const size = saved?.w ? { w: saved.w, h: saved.h } : cardSize;
  const onScreen = saved && screen.getAllDisplays().some(({ workArea: a }) =>
    saved.x < a.x + a.width && saved.x + size.w > a.x && saved.y < a.y + a.height && saved.y + size.h > a.y);
  if (!onScreen) return defaultCard();
  cardSize = size;
  return { x: saved.x, y: saved.y };
}

// The panel opens toward the middle of the display the card is on.
function updateAnchor() {
  const cx = card.x + cardSize.w / 2;
  const cy = card.y + cardSize.h / 2;
  const area = screen.getDisplayNearestPoint({ x: Math.round(cx), y: Math.round(cy) }).workArea;
  anchor = { right: cx > area.x + area.width / 2, bottom: cy > area.y + area.height / 2 };
}

// Window bounds for a box at `pos` of `size`, with PAD to spare around it.
const around = (pos, size) => ({
  x: Math.round(pos.x - PAD),
  y: Math.round(pos.y - PAD),
  width: Math.ceil(size.w + 2 * PAD),
  height: Math.ceil(size.h + 2 * PAD),
});

// Above/below the card when horizontal, beside it when vertical, aligned to the card edge facing the screen border.
function panelPos() {
  const { w, h } = panelSize;
  if (settings.vertical) {
    return {
      x: anchor.right ? card.x - GAP - w : card.x + cardSize.w + GAP,
      y: anchor.bottom ? card.y + cardSize.h - h : card.y,
    };
  }
  return {
    x: anchor.right ? card.x + cardSize.w - w : card.x,
    y: anchor.bottom ? card.y - GAP - h : card.y + cardSize.h + GAP,
  };
}

// Always setBounds, never setPosition: setPosition reuses the size read back from the window, which on a scaled
// display (125%, 150%) rounds up a pixel on every call, so a dragged window kept growing.
function layout() {
  cardWin.setBounds(around(card, cardSize));
  if (selected) panelWin.setBounds(around(panelPos(), panelSize));
}

function overlayWindow(view, bounds) {
  const win = new BrowserWindow({
    ...bounds,
    frame: false,
    transparent: true,
    resizable: false,
    skipTaskbar: true,
    focusable: view === 'panel',
    hasShadow: false,
    alwaysOnTop: true,
    show: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), zoomFactor: SCALE },
  });
  win.setAlwaysOnTop(true, 'screen-saver');
  win.setOpacity(settings.opacity);
  if (process.platform === 'darwin') win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  win.loadFile(path.join(__dirname, 'index.html'), { query: { view } });
  win.webContents.on('did-finish-load', () => {
    win.webContents.send('vertical', settings.vertical);
    if (netState) win.webContents.send('network', netState);
    if (last) win.webContents.send('quota', last);
  });
  if (view === 'card') win.once('ready-to-show', () => win.showInactive());
  return win;
}

function createWindows() {
  card = initialCard();
  updateAnchor();
  cardWin = overlayWindow('card', around(card, cardSize));
  // Only the open detail panel takes focus, so clicking another window dismisses it.
  panelWin = overlayWindow('panel', around(card, { w: 0, h: 0 }));
  panelWin.setIgnoreMouseEvents(true);
  panelWin.on('blur', () => { if (selected) select(null); });
}

// Opening a ring's panel: the panel page lays it out hidden and reports its size, the window is moved into place,
// and only then is the panel revealed, so it never shows up at its old spot first (see 'size').
function select(next) {
  selected = next;
  focusPanelOnReveal = !!next;
  for (const win of windows()) win.webContents.send('selected', next);
  if (!next) {
    panelWin.setIgnoreMouseEvents(true);
    panelWin.hide();
  }
}

async function refresh() {
  cardWin.webContents.send('loading');
  last = await fetchAll();
  for (const win of windows()) win.webContents.send('quota', last);
  if (process.env.QUOTA_OVERLAY_SNAPSHOT) snapshot(process.env.QUOTA_OVERLAY_SNAPSHOT);
  const lines = [['Claude', last.claude], ['Codex', last.codex], ['Cursor', last.cursor]]
    .filter(([, r]) => !r.absent)
    .map(([name, r]) => tipLine(name, r));
  tray.setToolTip((lines.join('\n') || 'Not logged in to Claude / Codex / Cursor').slice(0, 127));
}

// ms is the fastest answer. Coming back online refetches the quota, which failed while offline.
async function checkNetwork() {
  const start = Date.now();
  let online = false;
  try {
    await Promise.any(NET_URLS.map((url) =>
      net.fetch(url, { method: 'HEAD', cache: 'no-store', signal: AbortSignal.timeout(NET_TIMEOUT_MS) })));
    online = true;
  } catch {}
  const wasOffline = netState && !netState.online;
  netState = { online, ms: online ? Date.now() - start : null };
  cardWin.webContents.send('network', netState);
  if (online && wasOffline) refresh();
}

function tipLine(name, r) {
  if (!r.ok) return `${name}: ${r.error}`;
  return `${name} ` + r.windows.map((w) => `${w.label} ${Math.round(100 - w.usedPercent)}%`).join(' · ');
}

// Debug aid: save a PNG of the card after the first fetch, then exit.
function snapshot(file) {
  setTimeout(async () => {
    const img = await cardWin.webContents.capturePage();
    fs.writeFileSync(file, img.toPNG());
    app.quit();
  }, 1500);
}

function toggleVisible() {
  const show = !cardWin.isVisible();
  if (show) cardWin.showInactive();
  else {
    select(null);
    cardWin.hide();
  }
}

function resetPosition() {
  settings.card = null;
  saveSettings();
  card = defaultCard();
  updateAnchor();
  layout();
}

function toggleVertical() {
  settings.vertical = !settings.vertical;
  saveSettings();
  cardWin.webContents.send('vertical', settings.vertical);
}

function setOpacity(v) {
  settings.opacity = v;
  for (const win of windows()) win.setOpacity(v);
  saveSettings();
}

function restart() {
  saveSettings();
  app.relaunch();
  app.quit();
}

function menu() {
  const opacities = [1, 0.92, 0.8, 0.65, 0.5];
  return Menu.buildFromTemplate([
    { label: 'Refresh', click: refresh },
    { label: settings.vertical ? 'Rotate to horizontal' : 'Rotate to vertical', click: toggleVertical },
    { label: 'Snap to corner', click: resetPosition },
    { type: 'separator' },
    {
      label: 'Opacity',
      submenu: opacities.map((v) => ({
        label: `${Math.round(v * 100)}%`, type: 'radio', checked: settings.opacity === v, click: () => setOpacity(v),
      })),
    },
    // Login items are a Windows/macOS feature; on Linux use the desktop's autostart settings.
    ...(process.platform === 'linux' ? [] : [{
      label: 'Launch at startup',
      type: 'checkbox',
      checked: app.getLoginItemSettings(LOGIN_ITEM).openAtLogin,
      click: (item) => app.setLoginItemSettings({ ...LOGIN_ITEM, openAtLogin: item.checked }),
    }]),
    { type: 'separator' },
    { label: cardWin.isVisible() ? 'Hide    Ctrl+Alt+Q' : 'Show    Ctrl+Alt+Q', click: toggleVisible },
    { label: 'Restart', click: restart },
    { label: 'Quit', click: () => app.quit() },
  ]);
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => { cardWin?.showInactive(); });

  app.whenReady().then(() => {
    app.dock?.hide();
    loadSettings();
    createWindows();

    tray = new Tray(trayIcon());
    tray.setToolTip('Ai Quota Overlay');
    tray.on('click', toggleVisible);
    tray.on('right-click', () => tray.popUpContextMenu(menu()));
    // Linux trays have no click events; the menu must be attached up front.
    if (process.platform === 'linux') tray.setContextMenu(menu());

    globalShortcut.register('Control+Alt+Q', toggleVisible);

    // Each page reports the size of what it shows, and its window is fitted around it.
    ipcMain.on('size', (e, { w, h }) => {
      const size = { w: Math.ceil(w * SCALE), h: Math.ceil(h * SCALE) };
      if (e.sender === panelWin.webContents) {
        panelSize = size;
        if (!selected) return;
        panelWin.setBounds(around(panelPos(), panelSize));
        panelWin.setIgnoreMouseEvents(false);
        panelWin.webContents.send('reveal');
        if (focusPanelOnReveal) {
          focusPanelOnReveal = false;
          panelWin.show();
          panelWin.focus();
        }
        return;
      }
      if (size.w === cardSize.w && size.h === cardSize.h) return;
      // New data or rotation: keep the anchored corner where it is.
      if (anchor.right) card.x += cardSize.w - size.w;
      if (anchor.bottom) card.y += cardSize.h - size.h;
      cardSize = size;
      layout();
    });
    ipcMain.on('select', (_e, next) => select(next));
    ipcMain.on('refresh', refresh);
    ipcMain.on('menu', () => menu().popup({ window: cardWin }));
    // Drag is measured from the real cursor position, so page zoom and DPI don't skew it.
    ipcMain.on('drag-start', () => { dragFrom = { card: { ...card }, cursor: screen.getCursorScreenPoint() }; });
    ipcMain.on('drag-move', () => {
      if (!dragFrom) return;
      const p = screen.getCursorScreenPoint();
      card = { x: dragFrom.card.x + p.x - dragFrom.cursor.x, y: dragFrom.card.y + p.y - dragFrom.cursor.y };
      layout();
    });
    ipcMain.on('drag-end', () => {
      dragFrom = null;
      settings.card = { ...card, ...cardSize };
      saveSettings();
      updateAnchor();
      layout();
    });

    refresh();
    timer = setInterval(refresh, POLL_MS);
    checkNetwork();
    netTimer = setInterval(checkNetwork, NET_MS);
  });

  app.on('will-quit', () => {
    clearInterval(timer);
    clearInterval(netTimer);
    globalShortcut.unregisterAll();
  });
  app.on('window-all-closed', () => app.quit());
}
