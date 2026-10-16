const { app, BrowserWindow, Tray, Menu, nativeImage, globalShortcut, ipcMain, screen, net, powerMonitor } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { fetchAll } = require('./quota');
const I18N = require('./i18n');
const { createUpdater } = require('./updater');

// Quota: checked every POLL_MS while the card or the app window is on screen, every IDLE_POLL_MS while both are
// hidden. Checks never overlap, manual refreshes closer than MIN_REFRESH_GAP_MS reuse the last result, and a service
// that answers "too many requests" is left alone for BACKOFF_MS (doubling up to MAX_BACKOFF_MS) while it keeps
// showing its last good numbers, so checking often never turns into hammering anyone's API.
const POLL_MS = 60 * 1000;
const IDLE_POLL_MS = 5 * 60 * 1000;
const MIN_REFRESH_GAP_MS = 10 * 1000;
const BACKOFF_MS = 2 * 60 * 1000;
const MAX_BACKOFF_MS = 15 * 60 * 1000;
// Internet check: online when any of these answers within NET_TIMEOUT_MS. Any HTTP status counts, only a failed
// connection doesn't, so a captive portal or a blocked host still reads as online as long as one gets through.
// It is a single tiny HEAD request, skipped while the card is hidden or the previous one is still out.
const NET_MS = 3 * 1000;
const NET_TIMEOUT_MS = 2500;
const NET_URLS = ['https://www.gstatic.com/generate_204', 'https://1.1.1.1/cdn-cgi/trace'];
const MARGIN = 16;
// Whole UI is drawn at this zoom. Sizes from the page are CSS px, multiplied by SCALE for the windows.
const SCALE = 1.2;
// The card and the panel sit PAD inside their windows (room for the panel's outline) and GAP apart on screen.
const PAD = 2 * SCALE;
const GAP = 4 * SCALE;
// Run from source (`electron .`), a login item must also pass the app folder or it starts bare Electron.
const LOGIN_ITEM = process.defaultApp ? { args: [app.getAppPath()] } : {};
const SERVICES = [['claude', 'Claude'], ['codex', 'Codex'], ['cursor', 'Cursor']];
const SERVICE_KEYS = SERVICES.map(([key]) => key);
const LANG_KEYS = I18N.LANGS.map((l) => l.key);

const settingsFile = () => path.join(app.getPath('userData'), 'settings.json');
// card = where the card was left and its size then; null means "bottom-right of the primary display".
// services = which are checked at all; hidden = quota windows (by id) left off the card; seen = the windows each
// service had last time, so Settings can list them while a service is off. skipVersion = update the user skipped.
let settings = {
  card: null,
  opacity: 0.92,
  vertical: false,
  lang: 'en',
  network: false,
  services: { claude: true, codex: true, cursor: true },
  hidden: { claude: [], codex: [], cursor: [] },
  seen: {},
  skipVersion: null,
};
// The card and its detail panel get a window each, just big enough to hold them. A window takes the scale of the
// display holding most of it, so a small one rescales as soon as the card is dragged onto another monitor, and
// opening the panel never moves the card.
let cardWin;
let panelWin;
let settingsWin = null;
let toastWin;                          // "update available" toast, placed where the detail panel opens
let updater = null;
let tray;
let timer;
let netTimer = null;
let last = null;
let lastFetch = 0;                     // when the last quota check started
let refreshing = null;                 // the quota check under way, if any
const cooldown = {};                   // svc -> { until, ms } after a "too many requests"
let netBusy = false;
let netState = null;                   // { online, ms } from the last internet check; null while it's switched off
let card = null;                       // { x, y } on screen
let cardSize = { w: 192, h: 91 };
let panelSize = { w: 0, h: 0 };
let toastSize = { w: 0, h: 0 };
let selected = null;                   // { svc, index } of the ring whose panel is open
let focusPanelOnReveal = false;
let anchor = { right: true, bottom: true };
let dragFrom = null;

function loadSettings() {
  let saved = {};
  try { saved = JSON.parse(fs.readFileSync(settingsFile(), 'utf8')); } catch {}
  settings = {
    ...settings,
    ...saved,
    services: { ...settings.services, ...saved.services },
    hidden: { ...settings.hidden, ...saved.hidden },
    seen: { ...saved.seen },
  };
  if (!LANG_KEYS.includes(settings.lang)) settings.lang = 'en';
}
function saveSettings() {
  try { fs.writeFileSync(settingsFile(), JSON.stringify(settings)); } catch {}
}

const S = () => I18N.t(settings.lang);
const canStartAtLogin = process.platform !== 'linux';
const startAtLogin = () => canStartAtLogin && app.getLoginItemSettings(LOGIN_ITEM).openAtLogin;

// What the pages get to see of the settings.
const publicSettings = () => ({
  lang: settings.lang,
  vertical: settings.vertical,
  opacity: settings.opacity,
  network: settings.network,
  services: settings.services,
  hidden: settings.hidden,
  seen: settings.seen,
  startup: startAtLogin(),
  canStartAtLogin,
  visible: !!cardWin?.isVisible(),
  version: app.getVersion(),
});

const trayIcon = () => nativeImage.createFromPath(path.join(__dirname, 'icon.png')).resize({ width: 16, height: 16 });
const windows = () => [cardWin, panelWin, toastWin, settingsWin].filter((w) => w && !w.isDestroyed());
const broadcast = (channel, value) => { for (const win of windows()) win.webContents.send(channel, value); };

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
// The detail panel and the update toast both open there (the toast steps aside while the panel is open).
function popupPos({ w, h }) {
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
  if (selected) panelWin.setBounds(around(popupPos(panelSize), panelSize));
  layoutToast();
}

// The toast shows while an update is on offer or under way, unless it was skipped or closed, the card is hidden,
// or a detail panel is open in its spot. A failed check stays out of the way (the app window shows it).
const TOAST_PHASES = ['available', 'downloading', 'installing', 'error'];
function toastWanted() {
  const u = updater?.view();
  if (!u || !cardWin.isVisible() || selected || !TOAST_PHASES.includes(u.phase)) return false;
  if (u.phase === 'available') return !u.skipped && !u.dismissed;
  if (u.phase === 'error') return u.error?.during === 'download';
  return true;
}
function layoutToast() {
  if (!toastWin) return;
  if (!toastWanted() || !toastSize.w) return toastWin.hide();
  toastWin.setBounds(around(popupPos(toastSize), toastSize));
  if (!toastWin.isVisible()) toastWin.showInactive();
}

function sendState(win) {
  win.webContents.send('settings', publicSettings());
  if (netState) win.webContents.send('network', netState);
  if (last) win.webContents.send('quota', last);
  if (updater) win.webContents.send('update', updater.view());
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
  win.loadFile(path.join(__dirname, view === 'toast' ? 'toast.html' : 'index.html'), { query: { view } });
  win.webContents.on('did-finish-load', () => sendState(win));
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
  toastWin = overlayWindow('toast', around(card, { w: 0, h: 0 }));
}

// A normal window of its own, centred on the display the card is on.
function openSettings() {
  select(null);
  if (settingsWin && !settingsWin.isDestroyed()) {
    if (settingsWin.isMinimized()) settingsWin.restore();
    settingsWin.show();
    settingsWin.focus();
    updater?.checkIfStale();
    return;
  }
  const width = 440;
  const height = 760;
  const area = screen.getDisplayNearestPoint({ x: Math.round(card.x), y: Math.round(card.y) }).workArea;
  settingsWin = new BrowserWindow({
    width,
    height: Math.min(height, area.height - 2 * MARGIN),
    x: Math.round(area.x + (area.width - width) / 2),
    y: Math.round(area.y + (area.height - Math.min(height, area.height - 2 * MARGIN)) / 2),
    minWidth: 380,
    minHeight: 420,
    frame: false,
    maximizable: false,
    fullscreenable: false,
    backgroundColor: '#111318',
    title: `Ai Quota Overlay — ${S().s.title}`,
    icon: path.join(__dirname, 'icon.png'),
    show: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js') },
  });
  settingsWin.removeMenu();
  settingsWin.loadFile(path.join(__dirname, 'settings.html'));
  settingsWin.webContents.on('did-finish-load', () => sendState(settingsWin));
  settingsWin.once('ready-to-show', () => settingsWin.show());
  updater?.checkIfStale();
  settingsWin.on('closed', () => { settingsWin = null; });
}

// Opening a ring's panel: the panel page lays it out hidden and reports its size, the window is moved into place,
// and only then is the panel revealed, so it never shows up at its old spot first (see 'size').
function select(next) {
  selected = next;
  focusPanelOnReveal = !!next;
  for (const win of [cardWin, panelWin]) win.webContents.send('selected', next);
  if (!next) {
    panelWin.setIgnoreMouseEvents(true);
    panelWin.hide();
  }
  layoutToast();
}

// Remember each service's windows, so Settings can still list them while it's switched off or offline.
function rememberWindows() {
  let changed = false;
  for (const key of SERVICE_KEYS) {
    const r = last[key];
    if (!r?.ok || !r.windows.length) continue;
    const seen = r.windows.map(({ id, span, name, tag, billing }) => ({ id, span, name, tag, billing }));
    if (JSON.stringify(seen) === JSON.stringify(settings.seen[key])) continue;
    settings.seen[key] = seen;
    changed = true;
  }
  if (changed) {
    saveSettings();
    broadcast('settings', publicSettings());
  }
}

const onScreen = () => cardWin.isVisible() || !!(settingsWin && !settingsWin.isDestroyed() && settingsWin.isVisible());
const rateLimited = (r) => r?.error?.code === 'rateLimited' || r?.error?.status === 429;

// Runs every 5 s and checks once POLL_MS (on screen) or IDLE_POLL_MS (all hidden) has passed since the last check.
function tick() {
  if (Date.now() - lastFetch >= (onScreen() ? POLL_MS : IDLE_POLL_MS) - 1000) refresh({ force: true });
}

// force: skip the MIN_REFRESH_GAP_MS guard (timer, a service switched on, back online, wake from sleep).
async function refresh({ force = false } = {}) {
  if (refreshing) return refreshing;
  if (!force && last && Date.now() - lastFetch < MIN_REFRESH_GAP_MS) {
    broadcast('quota', last);
    return;
  }
  refreshing = (async () => {
    cardWin.webContents.send('loading');
    const now = Date.now();
    lastFetch = now;
    const cooling = SERVICE_KEYS.filter((k) => cooldown[k]?.until > now);
    const fresh = await fetchAll({ ...settings.services, ...Object.fromEntries(cooling.map((k) => [k, false])) });
    for (const k of SERVICE_KEYS) {
      if (settings.services[k] === false) delete cooldown[k];
      else if (cooling.includes(k)) fresh[k] = last?.[k] ?? fresh[k];
      else if (rateLimited(fresh[k])) {
        const ms = Math.min(MAX_BACKOFF_MS, (cooldown[k]?.ms ?? BACKOFF_MS / 2) * 2);
        cooldown[k] = { until: now + ms, ms };
        if (last?.[k]?.ok) fresh[k] = last[k];
      } else delete cooldown[k];
    }
    last = fresh;
    rememberWindows();
    broadcast('quota', last);
    if (process.env.QUOTA_OVERLAY_SNAPSHOT) snapshot(process.env.QUOTA_OVERLAY_SNAPSHOT);
    updateTooltip();
  })();
  try {
    await refreshing;
  } finally {
    refreshing = null;
  }
}

function updateTooltip() {
  if (!tray) return;
  if (!last) return tray.setToolTip('Ai Quota Overlay');
  const lines = SERVICES
    .filter(([key]) => last[key] && !last[key].absent)
    .map(([key, name]) => tipLine(key, name, last[key]));
  tray.setToolTip((lines.join('\n') || S().menu.notLoggedIn).slice(0, 127));
}

function tipLine(key, name, r) {
  if (!r.ok) return `${name}: ${I18N.errorText(r.error, settings.lang)}`;
  const hidden = settings.hidden[key] ?? [];
  return `${name} ` + r.windows
    .filter((w) => !hidden.includes(w.id))
    .map((w) => `${I18N.windowLabel(w, settings.lang, true)} ${Math.round(100 - w.usedPercent)}%`)
    .join(' · ');
}

// ms is the fastest answer. Coming back online refetches the quota, which failed while offline.
async function checkNetwork() {
  if (netBusy || !cardWin.isVisible()) return;
  netBusy = true;
  const start = Date.now();
  let online = false;
  try {
    await Promise.any(NET_URLS.map((url) =>
      net.fetch(url, { method: 'HEAD', cache: 'no-store', signal: AbortSignal.timeout(NET_TIMEOUT_MS) })));
    online = true;
  } catch {}
  netBusy = false;
  if (!settings.network) return; // switched off while the check was out
  const wasOffline = netState && !netState.online;
  netState = { online, ms: online ? Date.now() - start : null };
  broadcast('network', netState);
  if (online && wasOffline) refresh({ force: true });
}

// The internet check only runs while it's shown on the card (Settings → Internet status, off by default).
function applyNetwork() {
  clearInterval(netTimer);
  netTimer = null;
  if (settings.network) {
    checkNetwork();
    netTimer = setInterval(checkNetwork, NET_MS);
  } else {
    netState = null;
    broadcast('network', null);
  }
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
  layoutToast();
  broadcast('settings', publicSettings());
}

function resetPosition() {
  settings.card = null;
  saveSettings();
  card = defaultCard();
  updateAnchor();
  layout();
}

// Every change, from the app window or the shortcut, goes through here and is pushed to all pages.
function setSetting(key, value) {
  switch (key) {
    case 'lang':
      if (!LANG_KEYS.includes(value)) return;
      settings.lang = value;
      settingsWin?.setTitle(`Ai Quota Overlay — ${S().s.title}`);
      if (process.platform === 'linux') tray?.setContextMenu(linuxTrayMenu());
      break;
    case 'visible':
      if (!!value !== cardWin.isVisible()) toggleVisible();
      return;
    case 'vertical':
      settings.vertical = !!value;
      select(null);
      break;
    case 'opacity': {
      const v = Math.min(1, Math.max(0.3, Number(value) || 1));
      settings.opacity = Math.round(v * 100) / 100;
      for (const win of [cardWin, panelWin, toastWin]) win.setOpacity(settings.opacity);
      break;
    }
    case 'network':
      settings.network = !!value;
      applyNetwork();
      break;
    case 'startup':
      if (canStartAtLogin) app.setLoginItemSettings({ ...LOGIN_ITEM, openAtLogin: !!value });
      break;
    case 'service': {
      if (!SERVICE_KEYS.includes(value?.svc)) return;
      settings.services = { ...settings.services, [value.svc]: !!value.on };
      if (selected?.svc === value.svc) select(null);
      refresh({ force: true });
      break;
    }
    case 'window': {
      if (!SERVICE_KEYS.includes(value?.svc) || typeof value.id !== 'string') return;
      const hidden = new Set(settings.hidden[value.svc] ?? []);
      if (value.on) hidden.delete(value.id);
      else hidden.add(value.id);
      settings.hidden = { ...settings.hidden, [value.svc]: [...hidden] };
      break;
    }
    default:
      return;
  }
  saveSettings();
  broadcast('settings', publicSettings());
  updateTooltip();
}

function restart() {
  saveSettings();
  app.relaunch();
  app.quit();
}

// Linux trays have no click events, so there the tray keeps a menu, just enough to reach the app window.
function linuxTrayMenu() {
  const m = S().menu;
  return Menu.buildFromTemplate([
    { label: m.settings, click: openSettings },
    { type: 'separator' },
    { label: m.quit, click: () => app.quit() },
  ]);
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  // Starting the app again (Start menu, desktop shortcut) while it runs brings up the overlay and the app window.
  app.on('second-instance', () => {
    cardWin?.showInactive();
    openSettings();
  });

  app.whenReady().then(() => {
    app.dock?.hide();
    loadSettings();
    createWindows();

    // Every way into the app (tray left/right click, right-click on the card, "⋯") opens the app window;
    // everything the old menu did lives there now.
    tray = new Tray(trayIcon());
    tray.setToolTip('Ai Quota Overlay');
    tray.on('click', openSettings);
    tray.on('right-click', openSettings);
    if (process.platform === 'linux') tray.setContextMenu(linuxTrayMenu());

    globalShortcut.register('Control+Alt+Q', toggleVisible);

    // Each page reports the size of what it shows, and its window is fitted around it.
    ipcMain.on('size', (e, { w, h }) => {
      const size = { w: Math.ceil(w * SCALE), h: Math.ceil(h * SCALE) };
      if (e.sender === panelWin.webContents) {
        panelSize = size;
        if (!selected) return;
        panelWin.setBounds(around(popupPos(panelSize), panelSize));
        panelWin.setIgnoreMouseEvents(false);
        panelWin.webContents.send('reveal');
        if (focusPanelOnReveal) {
          focusPanelOnReveal = false;
          panelWin.show();
          panelWin.focus();
        }
        return;
      }
      if (e.sender === toastWin.webContents) {
        toastSize = size;
        layoutToast();
        return;
      }
      if (e.sender !== cardWin.webContents) return;
      if (size.w === cardSize.w && size.h === cardSize.h) return;
      // New data or rotation: keep the anchored corner where it is.
      if (anchor.right) card.x += cardSize.w - size.w;
      if (anchor.bottom) card.y += cardSize.h - size.h;
      cardSize = size;
      layout();
    });
    ipcMain.on('select', (_e, next) => select(next));
    ipcMain.on('refresh', () => refresh());
    ipcMain.on('open-settings', openSettings);
    ipcMain.on('restart', restart);
    ipcMain.on('quit', () => app.quit());
    // From the toast or the app window: check | download | skip | dismiss.
    ipcMain.on('update', (_e, action) => {
      if (['check', 'download', 'skip', 'dismiss'].includes(action)) updater[action]();
    });
    ipcMain.on('set', (_e, { key, value } = {}) => setSetting(key, value));
    ipcMain.on('snap', resetPosition);
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

    updater = createUpdater({
      getSkipped: () => settings.skipVersion,
      setSkipped: (version) => {
        settings.skipVersion = version;
        saveSettings();
      },
      onChange: (state) => {
        broadcast('update', state);
        layoutToast();
      },
    });

    refresh({ force: true });
    timer = setInterval(tick, 5 * 1000);
    applyNetwork();
    updater.start();
    // After sleep everything is stale: check it all again (each check still keeps its own guards).
    powerMonitor.on('resume', () => {
      setTimeout(() => {
        refresh({ force: true });
        checkNetwork();
        updater.checkIfStale(0);
      }, 5000);
    });
    if (process.env.QUOTA_OVERLAY_SETTINGS) openSettings();
  });

  app.on('will-quit', () => {
    clearInterval(timer);
    clearInterval(netTimer);
    updater?.stop();
    globalShortcut.unregisterAll();
  });
  // The settings window can close on its own; the overlay windows only go away with the app.
  app.on('window-all-closed', () => app.quit());
}
