const { app, BrowserWindow, Tray, Menu, nativeImage, globalShortcut, ipcMain, screen } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { fetchAll } = require('./quota');

const POLL_MS = 3 * 60 * 1000;
const MARGIN = 16;
// Whole UI is drawn at this zoom; sizes below and from the page are CSS px, multiplied by SCALE for the window.
const SCALE = 1.2;
// Linux can't forward mouse moves through an ignored window, so the transparent area stays solid there.
const CLICK_THROUGH = process.platform !== 'linux';
// Room kept free next to the card for the detail panel, so opening it never resizes the window.
// The unused part of the window is transparent and click-through.
const PANEL = { w: 256 * SCALE, h: 172 * SCALE };
const GAP = 4 * SCALE;
const PAD = 2 * SCALE;

const settingsFile = () => path.join(app.getPath('userData'), 'settings.json');
// card = screen position of the card's top-left corner; null means "bottom-right of the primary display".
let settings = { card: null, opacity: 0.92, vertical: false };
let win;
let tray;
let timer;
let last = null;
let card = null;                       // { x, y } on screen
let cardSize = { w: 192, h: 91 };
let anchor = { right: true, bottom: true };
let dragFrom = null;

function loadSettings() {
  try { settings = { ...settings, ...JSON.parse(fs.readFileSync(settingsFile(), 'utf8')) }; } catch {}
}
function saveSettings() {
  try { fs.writeFileSync(settingsFile(), JSON.stringify(settings)); } catch {}
}

const trayIcon = () => nativeImage.createFromPath(path.join(__dirname, 'icon.png')).resize({ width: 16, height: 16 });

function defaultCard() {
  const area = screen.getPrimaryDisplay().workArea;
  return { x: area.x + area.width - cardSize.w - MARGIN, y: area.y + area.height - cardSize.h - MARGIN };
}

function initialCard() {
  const saved = settings.card;
  const onScreen = saved && screen.getAllDisplays().some(({ workArea: a }) =>
    saved.x < a.x + a.width && saved.x + cardSize.w > a.x && saved.y < a.y + a.height && saved.y + cardSize.h > a.y);
  return onScreen ? { ...saved } : defaultCard();
}

// The card sits in the window corner nearest the screen edge; the panel room extends toward the middle.
function windowBounds() {
  const { w: cw, h: ch } = cardSize;
  const width = settings.vertical ? cw + GAP + PANEL.w + 2 * PAD : Math.max(cw, PANEL.w) + 2 * PAD;
  const height = settings.vertical ? Math.max(ch, PANEL.h) + 2 * PAD : ch + GAP + PANEL.h + 2 * PAD;
  return {
    x: Math.round(anchor.right ? card.x + cw + PAD - width : card.x - PAD),
    y: Math.round(anchor.bottom ? card.y + ch + PAD - height : card.y - PAD),
    width: Math.ceil(width),
    height: Math.ceil(height),
  };
}

function layout() {
  win.setBounds(windowBounds());
}

function updateAnchor() {
  const cx = card.x + cardSize.w / 2;
  const cy = card.y + cardSize.h / 2;
  const area = screen.getDisplayNearestPoint({ x: Math.round(cx), y: Math.round(cy) }).workArea;
  anchor = { right: cx > area.x + area.width / 2, bottom: cy > area.y + area.height / 2 };
  win.webContents.send('anchor', anchor);
}

function createWindow() {
  card = initialCard();
  win = new BrowserWindow({
    x: 0, y: 0, width: 100, height: 100,
    frame: false,
    transparent: true,
    resizable: false,
    skipTaskbar: true,
    focusable: false,
    hasShadow: false,
    alwaysOnTop: true,
    show: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js') },
  });
  updateAnchor();
  layout();
  win.setAlwaysOnTop(true, 'screen-saver');
  win.setOpacity(settings.opacity);
  if (CLICK_THROUGH) win.setIgnoreMouseEvents(true, { forward: true });
  if (process.platform === 'darwin') win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  win.loadFile(path.join(__dirname, 'index.html'));
  win.webContents.on('did-finish-load', () => {
    win.webContents.setZoomFactor(SCALE);
    win.webContents.send('anchor', anchor);
    win.webContents.send('layout', { vertical: settings.vertical });
    if (last) win.webContents.send('quota', last);
  });
  win.once('ready-to-show', () => win.showInactive());
}

async function refresh() {
  win?.webContents.send('loading', true);
  last = await fetchAll();
  win?.webContents.send('quota', last);
  if (process.env.QUOTA_OVERLAY_SNAPSHOT) snapshot(process.env.QUOTA_OVERLAY_SNAPSHOT);
  const lines = [['Claude', last.claude], ['Codex', last.codex], ['Cursor', last.cursor]]
    .filter(([, r]) => !r.absent)
    .map(([name, r]) => tipLine(name, r));
  tray?.setToolTip((lines.join('\n') || 'Not logged in to Claude / Codex / Cursor').slice(0, 127));
}

function tipLine(name, r) {
  if (!r.ok) return `${name}: ${r.error}`;
  return `${name} ` + r.windows.map((w) => `${w.label} ${Math.round(100 - w.usedPercent)}%`).join(' · ');
}

// Debug aid: save a PNG of the overlay after the first fetch, then exit.
function snapshot(file) {
  setTimeout(async () => {
    const img = await win.webContents.capturePage();
    fs.writeFileSync(file, img.toPNG());
    app.quit();
  }, 1500);
}

function toggleVisible() {
  if (win.isVisible()) win.hide();
  else win.showInactive();
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
  win.webContents.send('layout', { vertical: settings.vertical });
}

function setOpacity(v) {
  settings.opacity = v;
  win.setOpacity(v);
  saveSettings();
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
      checked: app.getLoginItemSettings().openAtLogin,
      click: (item) => app.setLoginItemSettings({ openAtLogin: item.checked }),
    }]),
    { type: 'separator' },
    { label: win?.isVisible() ? 'Hide    Ctrl+Alt+Q' : 'Show    Ctrl+Alt+Q', click: toggleVisible },
    { label: 'Quit', click: () => app.quit() },
  ]);
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => win?.showInactive());

  app.whenReady().then(() => {
    app.dock?.hide();
    loadSettings();
    createWindow();

    tray = new Tray(trayIcon());
    tray.setToolTip('Ai Quota Overlay');
    tray.on('click', toggleVisible);
    tray.on('right-click', () => tray.popUpContextMenu(menu()));
    // Linux trays have no click events; the menu must be attached up front.
    if (process.platform === 'linux') tray.setContextMenu(menu());

    globalShortcut.register('Control+Alt+Q', toggleVisible);

    // Card content changed size (new data or rotation): keep its anchored corner where it is.
    ipcMain.on('card-size', (_e, { w, h }) => {
      w = Math.ceil(w * SCALE);
      h = Math.ceil(h * SCALE);
      if (w === cardSize.w && h === cardSize.h) return;
      if (anchor.right) card.x += cardSize.w - w;
      if (anchor.bottom) card.y += cardSize.h - h;
      cardSize = { w, h };
      layout();
    });
    // Mouse over the transparent part of the window passes through to whatever is below.
    ipcMain.on('interactive', (_e, on) => { if (CLICK_THROUGH) win.setIgnoreMouseEvents(!on, { forward: true }); });
    ipcMain.on('refresh', refresh);
    ipcMain.on('menu', () => menu().popup({ window: win }));
    // Drag is measured from the real cursor position, so page zoom and DPI don't skew it.
    ipcMain.on('drag-start', () => { dragFrom = { card: { ...card }, cursor: screen.getCursorScreenPoint() }; });
    ipcMain.on('drag-move', () => {
      if (!dragFrom) return;
      const p = screen.getCursorScreenPoint();
      card = { x: dragFrom.card.x + p.x - dragFrom.cursor.x, y: dragFrom.card.y + p.y - dragFrom.cursor.y };
      const b = windowBounds();
      win.setPosition(b.x, b.y);
    });
    ipcMain.on('drag-end', () => {
      dragFrom = null;
      settings.card = { ...card };
      saveSettings();
      updateAnchor();
      layout();
    });

    refresh();
    timer = setInterval(refresh, POLL_MS);
  });

  app.on('will-quit', () => {
    clearInterval(timer);
    globalShortcut.unregisterAll();
  });
  app.on('window-all-closed', () => app.quit());
}
