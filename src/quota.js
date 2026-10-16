// Fetches remaining quota for Claude (claude.ai subscription), Codex (ChatGPT plan) and Cursor.
// Credentials are read from where the official apps already keep them; nothing is stored elsewhere.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const HOME = os.homedir();
const CLAUDE_CREDS = path.join(process.env.CLAUDE_CONFIG_DIR ?? path.join(HOME, '.claude'), '.credentials.json');
const CODEX_AUTH = path.join(process.env.CODEX_HOME ?? path.join(HOME, '.codex'), 'auth.json');
const CURSOR_DB = path.join(
  {
    win32: process.env.APPDATA ?? path.join(HOME, 'AppData', 'Roaming'),
    darwin: path.join(HOME, 'Library', 'Application Support'),
  }[process.platform] ?? process.env.XDG_CONFIG_HOME ?? path.join(HOME, '.config'),
  'Cursor', 'User', 'globalStorage', 'state.vscdb',
);

// Public OAuth client of Claude Code; used only to refresh the user's own token.
const CLAUDE_CLIENT_ID = '9d1c250a-e61b-44d9-88ed-5944d1962f5e';
const CLAUDE_TOKEN_URLS = [
  'https://platform.claude.com/v1/oauth/token',
  'https://console.anthropic.com/v1/oauth/token',
];

// Windows are language-neutral: an id, a span in seconds and a model name or tag, worded by src/i18n.js.
const H = 3600;
const CLAUDE_WINDOWS = {
  five_hour: { span: 5 * H },
  seven_day: { span: 168 * H },
  seven_day_opus: { span: 168 * H, name: 'Opus' },
  seven_day_sonnet: { span: 168 * H, name: 'Sonnet' },
  seven_day_oauth_apps: { span: 168 * H, tag: 'otherApps' },
};

// code names the message in src/i18n.js (errors.*); params fill it in.
class QuotaError extends Error {
  constructor(code, params = {}) {
    super(code);
    this.code = code;
    this.params = params;
  }
}
// Thrown when a service has no login on this machine at all; the overlay hides it.
class NotLoggedIn extends Error {}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function writeJsonAtomic(file, data) {
  const tmp = `${file}.quota-overlay.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(data), { mode: 0o600 });
  fs.renameSync(tmp, file);
}

async function refreshClaudeToken(oauth) {
  let lastStatus = 0;
  for (const url of CLAUDE_TOKEN_URLS) {
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        grant_type: 'refresh_token',
        refresh_token: oauth.refreshToken,
        client_id: CLAUDE_CLIENT_ID,
      }),
    }).catch(() => null);
    if (!res) continue;
    lastStatus = res.status;
    if (!res.ok) continue;
    const t = await res.json();

    // Re-read so a refresh done meanwhile by the CLI is not overwritten.
    const current = readJson(CLAUDE_CREDS);
    if (current.claudeAiOauth?.refreshToken !== oauth.refreshToken) {
      return current.claudeAiOauth.accessToken;
    }
    current.claudeAiOauth = {
      ...current.claudeAiOauth,
      accessToken: t.access_token,
      refreshToken: t.refresh_token ?? oauth.refreshToken,
      expiresAt: Date.now() + (t.expires_in ?? 3600) * 1000,
    };
    writeJsonAtomic(CLAUDE_CREDS, current);
    return t.access_token;
  }
  throw lastStatus ? new QuotaError('claudeExpired', { status: lastStatus }) : new QuotaError('offline');
}

// Claude Code keeps its login in ~/.claude/.credentials.json on Windows/Linux and in the Keychain on macOS.
function readClaudeOauth() {
  if (fs.existsSync(CLAUDE_CREDS)) return { oauth: readJson(CLAUDE_CREDS).claudeAiOauth, source: 'file' };
  if (process.platform === 'darwin') {
    try {
      const raw = execFileSync('security', ['find-generic-password', '-s', 'Claude Code-credentials', '-w'],
        { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], timeout: 5000 });
      return { oauth: JSON.parse(raw).claudeAiOauth, source: 'keychain' };
    } catch {}
  }
  throw new NotLoggedIn();
}

async function claudeAccessToken(force = false) {
  const { oauth, source } = readClaudeOauth();
  if (!oauth?.accessToken) throw new NotLoggedIn();
  if (!force && oauth.expiresAt - Date.now() > 60_000) return oauth.accessToken;
  // Refreshing rotates the refresh token; only do it where the new one can be written back for the CLI.
  if (source !== 'file') throw new QuotaError('claudeExpiredKeychain');
  return refreshClaudeToken(oauth);
}

async function fetchClaude() {
  const plan = readClaudeOauth().oauth?.subscriptionType ?? '';
  const call = (token) => fetch('https://api.anthropic.com/api/oauth/usage', {
    headers: { Authorization: `Bearer ${token}`, 'anthropic-beta': 'oauth-2025-04-20' },
  });

  let res = await call(await claudeAccessToken());
  if (res.status === 401) res = await call(await claudeAccessToken(true));
  if (res.status === 429) throw new QuotaError('rateLimited');
  if (!res.ok) throw new QuotaError('status', { service: 'Claude', status: res.status });
  const data = await res.json();

  const windows = Object.entries(data)
    // Unknown internal buckets without a reset time are placeholders, not real limits.
    .filter(([key, w]) => typeof w?.utilization === 'number' && (CLAUDE_WINDOWS[key] || w.resets_at))
    .map(([key, w]) => ({
      id: key,
      ...(CLAUDE_WINDOWS[key] ?? { name: key.replaceAll('_', ' ') }),
      usedPercent: w.utilization,
      resetsAt: w.resets_at ? Date.parse(w.resets_at) : null,
    }));
  const extra = data.extra_usage?.is_enabled
    ? { used: data.extra_usage.used_credits, limit: data.extra_usage.monthly_limit }
    : null;
  return { plan, windows, extra };
}

async function fetchCodex() {
  if (!fs.existsSync(CODEX_AUTH)) throw new NotLoggedIn();
  const auth = readJson(CODEX_AUTH);
  if (!auth.tokens?.access_token) throw new NotLoggedIn();

  const res = await fetch('https://chatgpt.com/backend-api/wham/usage', {
    headers: {
      Authorization: `Bearer ${auth.tokens.access_token}`,
      'chatgpt-account-id': auth.tokens.account_id,
      'User-Agent': 'codex_cli_rs',
    },
  });
  if (res.status === 401) throw new QuotaError('codexExpired');
  if (!res.ok) throw new QuotaError('status', { service: 'Codex', status: res.status });
  const data = await res.json();

  const rl = data.rate_limit ?? {};
  const windows = [['primary', rl.primary_window], ['secondary', rl.secondary_window]]
    .filter(([, w]) => w)
    .map(([id, w]) => ({
      id,
      span: w.limit_window_seconds,
      usedPercent: w.used_percent,
      resetsAt: w.reset_at ? w.reset_at * 1000 : Date.now() + w.reset_after_seconds * 1000,
    }));
  const credits = data.credits?.has_credits && !data.credits.unlimited
    ? { balance: Number(data.credits.balance) }
    : null;
  return { plan: data.plan_type ?? '', windows, credits, limitReached: !!rl.limit_reached };
}

function readCursorAuth() {
  if (!fs.existsSync(CURSOR_DB)) throw new NotLoggedIn();
  // Loaded lazily: node:sqlite is only needed when Cursor is installed.
  const { DatabaseSync } = require('node:sqlite');
  const db = new DatabaseSync(CURSOR_DB, { readOnly: true });
  try {
    const get = (key) => db.prepare('select value from ItemTable where key = ?').get(key)?.value;
    const token = get('cursorAuth/accessToken');
    if (!token) throw new NotLoggedIn();
    return { token: String(token), plan: String(get('cursorAuth/stripeMembershipType') ?? '') };
  } finally {
    db.close();
  }
}

async function fetchCursor() {
  const { token, plan } = readCursorAuth();
  const payload = JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString());
  if (payload.exp && payload.exp * 1000 < Date.now()) throw new QuotaError('cursorExpired');
  const userId = String(payload.sub).split('|').pop();
  const headers = { Cookie: `WorkosCursorSessionToken=${userId}%3A%3A${token}`, Origin: 'https://cursor.com' };

  // Current (usage-based) plans.
  const res = await fetch('https://cursor.com/api/usage-summary', { headers });
  if (res.status === 401 || res.status === 403) throw new QuotaError('cursorExpired');
  if (res.ok) {
    const data = await res.json();
    const p = data.individualUsage?.plan;
    const windows = p ? [{
      id: 'billing',
      billing: true,
      usedPercent: typeof p.totalPercentUsed === 'number' ? p.totalPercentUsed : p.limit ? (p.used / p.limit) * 100 : 0,
      resetsAt: data.billingCycleEnd ? Date.parse(data.billingCycleEnd) : null,
    }] : [];
    const od = data.individualUsage?.onDemand;
    const extra = od?.enabled && od.limit ? { used: od.used, limit: od.limit } : null;
    return { plan: data.membershipType ?? plan, windows, extra };
  }

  // Older request-count plans: one window per model, all resetting a month after startOfMonth.
  const old = await fetch(`https://cursor.com/api/usage?user=${encodeURIComponent(userId)}`, { headers });
  if (!old.ok) throw new QuotaError('status', { service: 'Cursor', status: `${res.status}/${old.status}` });
  const data = await old.json();
  const reset = data.startOfMonth ? new Date(data.startOfMonth) : null;
  reset?.setMonth(reset.getMonth() + 1);
  const windows = Object.entries(data)
    .filter(([, u]) => u?.maxRequestUsage)
    .map(([model, u]) => ({
      id: model, name: model, usedPercent: (u.numRequests / u.maxRequestUsage) * 100, resetsAt: reset?.getTime() ?? null,
    }));
  return { plan, windows, extra: null };
}

async function settle(fn) {
  try {
    return { ok: true, ...(await fn()) };
  } catch (e) {
    if (e instanceof NotLoggedIn) return { ok: false, absent: true };
    if (e instanceof TypeError && e.message === 'fetch failed') return { ok: false, error: { code: 'offline' } };
    if (e instanceof QuotaError) return { ok: false, error: { code: e.code, ...e.params } };
    return { ok: false, error: { code: 'unexpected', message: e.message } };
  }
}

// enabled = { cursor: false, … } skips a service switched off in Settings; it comes back as disabled.
// QUOTA_OVERLAY_SERVICES=claude,codex also limits which services are checked (default: all).
const FETCHERS = { claude: fetchClaude, codex: fetchCodex, cursor: fetchCursor };

async function fetchAll(enabled = {}) {
  const only = process.env.QUOTA_OVERLAY_SERVICES?.toLowerCase().split(',').map((s) => s.trim());
  const skip = (key) => enabled[key] === false || (only && !only.includes(key));
  const entries = await Promise.all(Object.entries(FETCHERS).map(async ([key, fn]) =>
    [key, skip(key) ? { ok: false, absent: true, disabled: true } : await settle(fn)]));
  return { ...Object.fromEntries(entries), fetchedAt: Date.now() };
}

module.exports = { fetchAll };
