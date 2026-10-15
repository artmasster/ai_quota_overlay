// UI text in English and Thai, shared by main (require) and the pages (<script src="i18n.js">, as window.I18N).
// Quota data is language-neutral: windows carry a span in seconds and errors a code, and are worded here.
(function (root) {
  const STRINGS = {
    en: {
      locale: 'en-US',
      // card
      net: 'Net',
      online: 'Connected to the internet',
      offline: 'Offline',
      offlineTitle: 'No internet connection',
      noData: 'no data',
      error: 'error',
      notLoggedIn: 'Not logged in to Claude, Codex or Cursor',
      allOff: 'Nothing to show — pick services in Settings',
      more: 'Settings',
      // ring labels (time left), long and short window names
      leftDays: (d) => `${d}d`,
      leftHours: (h) => `${h}h`,
      leftMinutes: (m) => `${m}m`,
      spanDays: (d) => `${d} days`,
      spanHours: (h) => `${h} h`,
      spanDaysShort: (d) => `${d}d`,
      spanHoursShort: (h) => `${h}h`,
      billing: 'Billing cycle',
      billingShort: 'Bill',
      otherApps: 'Other apps',
      // detail panel
      close: 'Close',
      resetsIn: 'Resets in',
      until: (d, h, m) => (d ? `${d} d ${h} h` : h ? `${h} h ${m} min` : `${m} min`),
      today: 'Today',
      tomorrow: 'Tomorrow',
      leftUsed: (p) => `left · ${p}% used`,
      creditsLeft: 'Credits left',
      extraUsage: 'Extra usage',
      noQuota: 'No quota data',
      retryHint: 'Double-click the card to try again',
      // errors from quota.js
      errors: {
        offline: () => "Can't reach the internet",
        claudeExpired: ({ status }) => `Login expired (${status}) — run claude in a terminal, then /login`,
        claudeExpiredKeychain: () => 'Login expired — open claude in a terminal once',
        rateLimited: () => 'Checking too often, try again shortly',
        status: ({ service, status }) => `${service} returned ${status}`,
        codexExpired: () => 'Login expired — open the Codex app once',
        cursorExpired: () => 'Login expired — open the Cursor app once',
        unexpected: ({ message }) => `Error: ${message}`,
      },
      // menus
      menu: {
        settings: 'Settings…',
        refresh: 'Refresh',
        toHorizontal: 'Rotate to horizontal',
        toVertical: 'Rotate to vertical',
        snap: 'Snap to corner',
        opacity: 'Opacity',
        startup: 'Launch at startup',
        hide: 'Hide',
        show: 'Show',
        restart: 'Restart',
        quit: 'Quit',
        notLoggedIn: 'Not logged in to Claude / Codex / Cursor',
      },
      // settings window
      s: {
        title: 'Settings',
        general: 'General',
        language: 'Language',
        layout: 'Layout',
        horizontal: 'Horizontal',
        vertical: 'Vertical',
        opacity: 'Opacity',
        startup: 'Launch at startup',
        startupHint: 'Open the overlay when you sign in',
        network: 'Internet status',
        networkShow: 'Show internet status',
        networkHint: 'Pings every 5 seconds and shows the latency on the card',
        services: 'Services',
        servicesHint: 'Pick which services and which quota windows show on the card',
        connected: 'Connected',
        absent: 'Not logged in',
        failed: 'Error',
        disabled: 'Off',
        checking: 'Checking…',
        windowsPending: 'Quota windows show up here after the first check',
        windowsOff: 'Turn on to check this service',
        login: {
          claude: 'Run claude in a terminal, then /login',
          codex: 'Sign in to Codex with your ChatGPT account',
          cursor: 'Sign in to the Cursor app',
        },
        refresh: 'Refresh now',
        snap: 'Snap to corner',
        shortcut: 'Show / hide',
        updated: (time) => `Updated ${time}`,
      },
    },
    th: {
      locale: 'th-TH',
      net: 'เน็ต',
      online: 'เชื่อมต่ออินเทอร์เน็ตได้',
      offline: 'ออฟไลน์',
      offlineTitle: 'เชื่อมต่ออินเทอร์เน็ตไม่ได้',
      noData: 'ไม่มีข้อมูล',
      error: 'ผิดพลาด',
      notLoggedIn: 'ยังไม่ได้ login Claude, Codex หรือ Cursor',
      allOff: 'ไม่มีอะไรให้แสดง — เลือกบริการในหน้าตั้งค่า',
      more: 'ตั้งค่า',
      leftDays: (d) => `${d} วัน`,
      leftHours: (h) => `${h} ชม`,
      leftMinutes: (m) => `${m} นาที`,
      spanDays: (d) => `${d} วัน`,
      spanHours: (h) => `${h} ชม.`,
      spanDaysShort: (d) => `${d}ว`,
      spanHoursShort: (h) => `${h}ช`,
      billing: 'รอบบิล',
      billingShort: 'บิล',
      otherApps: 'แอปอื่น',
      close: 'ปิด',
      resetsIn: 'รีเซ็ตใน',
      until: (d, h, m) => (d ? `${d} วัน ${h} ชม.` : h ? `${h} ชม. ${m} นาที` : `${m} นาที`),
      today: 'วันนี้',
      tomorrow: 'พรุ่งนี้',
      leftUsed: (p) => `เหลือ · ใช้ไป ${p}%`,
      creditsLeft: 'เครดิตเหลือ',
      extraUsage: 'ใช้เกินแพ็กเกจ',
      noQuota: 'ไม่มีข้อมูลโควต้า',
      retryHint: 'ดับเบิลคลิกที่กล่องเพื่อลองใหม่',
      errors: {
        offline: () => 'ต่ออินเทอร์เน็ตไม่ได้',
        claudeExpired: ({ status }) => `login หมดอายุ (${status}) — เปิด terminal พิมพ์ claude แล้ว /login`,
        claudeExpiredKeychain: () => 'login หมดอายุ — เปิด claude ใน terminal สักครั้ง',
        rateLimited: () => 'ถูกจำกัดการเช็ค รอสักครู่',
        status: ({ service, status }) => `${service} ตอบ ${status}`,
        codexExpired: () => 'login หมดอายุ — เปิดแอป Codex สักครั้ง',
        cursorExpired: () => 'login หมดอายุ — เปิดแอป Cursor สักครั้ง',
        unexpected: ({ message }) => `ผิดพลาด: ${message}`,
      },
      menu: {
        settings: 'ตั้งค่า…',
        refresh: 'รีเฟรช',
        toHorizontal: 'หมุนเป็นแนวนอน',
        toVertical: 'หมุนเป็นแนวตั้ง',
        snap: 'กลับไปมุมจอ',
        opacity: 'ความทึบ',
        startup: 'เปิดพร้อมเครื่อง',
        hide: 'ซ่อน',
        show: 'แสดง',
        restart: 'รีสตาร์ท',
        quit: 'ปิดแอป',
        notLoggedIn: 'ยังไม่ได้ login Claude / Codex / Cursor',
      },
      s: {
        title: 'ตั้งค่า',
        general: 'ทั่วไป',
        language: 'ภาษา',
        layout: 'การวาง',
        horizontal: 'แนวนอน',
        vertical: 'แนวตั้ง',
        opacity: 'ความทึบ',
        startup: 'เปิดพร้อมเครื่อง',
        startupHint: 'เปิด overlay เองตอนเข้าเครื่อง',
        network: 'สถานะอินเทอร์เน็ต',
        networkShow: 'แสดงสถานะอินเทอร์เน็ต',
        networkHint: 'เช็คทุก 5 วินาที และแสดงค่า ping บนกล่อง',
        services: 'บริการ',
        servicesHint: 'เลือกว่าจะให้บริการไหน และรอบโควต้าไหนขึ้นบนกล่อง',
        connected: 'เชื่อมต่อแล้ว',
        absent: 'ยังไม่ได้ login',
        failed: 'ผิดพลาด',
        disabled: 'ปิดอยู่',
        checking: 'กำลังเช็ค…',
        windowsPending: 'รอบโควต้าจะขึ้นตรงนี้หลังเช็คครั้งแรก',
        windowsOff: 'เปิดเพื่อเช็คบริการนี้',
        login: {
          claude: 'เปิด terminal พิมพ์ claude แล้ว /login',
          codex: 'login Codex ด้วยบัญชี ChatGPT',
          cursor: 'login แอป Cursor',
        },
        refresh: 'รีเฟรชตอนนี้',
        snap: 'กลับไปมุมจอ',
        shortcut: 'ซ่อน / แสดง',
        updated: (time) => `อัปเดต ${time}`,
      },
    },
  };

  const LANGS = [{ key: 'en', name: 'English' }, { key: 'th', name: 'ไทย' }];
  const t = (lang) => STRINGS[lang] ?? STRINGS.en;

  // "5 h", "7 days · Opus", "Billing cycle"; short = "5h", "7d", "Bill" for under a ring.
  function windowLabel(w, lang, short = false) {
    const s = t(lang);
    if (w.billing) return short ? s.billingShort : s.billing;
    let span = null;
    if (w.span) {
      const h = Math.round(w.span / 3600);
      span = h % 24 === 0
        ? (short ? s.spanDaysShort : s.spanDays)(h / 24)
        : (short ? s.spanHoursShort : s.spanHours)(h);
    }
    const name = w.tag ? s[w.tag] : w.name;
    return [span, name].filter(Boolean).join(' · ') || w.id;
  }

  function errorText(err, lang) {
    if (!err) return '';
    if (typeof err === 'string') return err;
    const fn = t(lang).errors[err.code] ?? t(lang).errors.unexpected;
    return fn(err);
  }

  const I18N = { STRINGS, LANGS, t, windowLabel, errorText };
  if (typeof module !== 'undefined' && module.exports) module.exports = I18N;
  else root.I18N = I18N;
})(this);
