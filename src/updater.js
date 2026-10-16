// Updates from GitHub Releases (latest.yml / latest-linux.yml written by electron-builder). Nothing downloads on its
// own: a newer version is offered in a small toast, and Update downloads it, installs it silently and restarts.
// Skip remembers that version and stays quiet until a newer one comes out.
//
// macOS builds aren't signed, and an unsigned app can't replace itself there, so on macOS the check reads the latest
// release tag and Update opens the release page. Run from source nothing is checked, unless
// QUOTA_OVERLAY_FAKE_UPDATE=1.9.0 is set: that pretends 1.9.0 is out and plays the whole flow (download progress
// included) without installing anything.
const { app, shell } = require('electron');

// First look shortly after start, then every 15 minutes, plus whenever the app window opens or the PC wakes up if
// the last look is older than STALE_MS. GitHub serves this from its release feed; 4–5 looks an hour is nothing to it.
const CHECK_DELAY_MS = 5 * 1000;
const CHECK_EVERY_MS = 15 * 60 * 1000;
const STALE_MS = 2 * 60 * 1000;
const RELEASES_URL = 'https://github.com/artmasster/ai_quota_overlay/releases/latest';
const LATEST_API = 'https://api.github.com/repos/artmasster/ai_quota_overlay/releases/latest';

// "1.10.0" > "1.9.2"; anything after a "-" (pre-release tags) is ignored.
function newer(a, b) {
  const parts = (v) => v.split('-')[0].split('.').map((n) => Number(n) || 0);
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  }
  return false;
}

// phase: idle | checking | latest | available | downloading | installing | error | unsupported
// error.during = 'check' (shown only in the app window) or 'download' (shown in the toast too).
function createUpdater({ getSkipped, setSkipped, onChange }) {
  const fake = process.env.QUOTA_OVERLAY_FAKE_UPDATE;
  const manual = process.platform === 'darwin';
  let state = { phase: 'idle', current: app.getVersion(), manual };
  let dismissed = null; // version closed with × for this run
  let timer = null;
  let lastCheck = 0;
  let updater = null;

  const set = (patch) => {
    state = { ...state, ...patch };
    onChange(view());
  };
  const view = () => ({
    ...state,
    skipped: !!state.version && state.version === getSkipped(),
    dismissed: !!state.version && state.version === dismissed,
  });

  if (!app.isPackaged && !fake) {
    state.phase = 'unsupported';
  } else if (!fake && !manual) {
    ({ autoUpdater: updater } = require('electron-updater'));
    updater.autoDownload = false;
    updater.autoInstallOnAppQuit = false;
    updater.logger = null;
    // A re-check while an update is already on offer leaves the toast alone instead of making it blink.
    updater.on('checking-for-update', () => { if (state.phase !== 'available') set({ phase: 'checking', error: null }); });
    updater.on('update-available', (info) => set({ phase: 'available', version: info.version, percent: 0 }));
    updater.on('update-not-available', () => set({ phase: 'latest' }));
    updater.on('download-progress', (p) => set({ phase: 'downloading', percent: Math.floor(p.percent) }));
    updater.on('update-downloaded', () => install());
    updater.on('error', (e) => set({
      phase: 'error',
      error: { during: state.phase === 'downloading' ? 'download' : 'check', message: String(e?.message ?? e).split('\n')[0] },
    }));
  }

  async function check() {
    if (['unsupported', 'checking', 'downloading', 'installing'].includes(state.phase)) return;
    lastCheck = Date.now();
    if (fake) {
      if (state.phase !== 'available') set({ phase: 'checking', error: null });
      setTimeout(() => set({ phase: 'available', version: fake, percent: 0 }), 800);
      return;
    }
    if (manual) return checkLatestTag();
    try { await updater.checkForUpdates(); } catch {} // reported through 'error'
    if (state.phase === 'checking') set({ phase: 'idle' }); // no answer either way; try again next time
  }

  // macOS: only the version is needed, so ask GitHub for the latest release's tag.
  async function checkLatestTag() {
    if (state.phase !== 'available') set({ phase: 'checking', error: null });
    try {
      const res = await fetch(LATEST_API, { headers: { Accept: 'application/vnd.github+json' } });
      if (!res.ok) throw new Error(`GitHub ${res.status}`);
      const version = String((await res.json()).tag_name ?? '').replace(/^v/, '');
      if (newer(version, state.current)) set({ phase: 'available', version });
      else set({ phase: 'latest' });
    } catch (e) {
      set({ phase: 'error', error: { during: 'check', message: e.message } });
    }
  }

  async function download() {
    if (!state.version || state.phase === 'downloading' || state.phase === 'installing') return;
    if (manual) return shell.openExternal(RELEASES_URL);
    set({ phase: 'downloading', percent: 0, error: null });
    if (fake) {
      const tick = setInterval(() => {
        if (state.percent >= 100) {
          clearInterval(tick);
          install();
        } else set({ percent: Math.min(100, state.percent + 7) });
      }, 180);
      return;
    }
    try { await updater.downloadUpdate(); } catch {} // reported through 'error'
  }

  // Silent install, then the installer starts the new version.
  function install() {
    set({ phase: 'installing', percent: 100 });
    setTimeout(() => {
      if (fake) return set({ phase: 'latest', current: state.version, version: null });
      updater.quitAndInstall(true, true);
    }, 1200);
  }

  function skip() {
    if (!state.version) return;
    setSkipped(state.version);
    onChange(view());
  }

  function dismiss() {
    dismissed = state.version;
    if (state.phase === 'error') state = { ...state, phase: state.version ? 'available' : 'idle', error: null };
    onChange(view());
  }

  function checkIfStale(maxAge = STALE_MS) {
    if (Date.now() - lastCheck >= maxAge) check();
  }

  function start() {
    if (state.phase === 'unsupported') return;
    setTimeout(check, fake ? 2000 : CHECK_DELAY_MS);
    timer = setInterval(check, CHECK_EVERY_MS);
  }

  return { start, check, checkIfStale, download, skip, dismiss, view, stop: () => clearInterval(timer) };
}

module.exports = { createUpdater };
