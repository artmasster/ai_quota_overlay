//! UI text in English and Thai (port of src/i18n.js). Quota data is language-neutral: windows carry a span in
//! seconds and errors a code, and are worded here.
use crate::model::{QErr, Win};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Th,
}

impl Lang {
    pub fn from_key(k: &str) -> Option<Lang> {
        match k {
            "en" => Some(Lang::En),
            "th" => Some(Lang::Th),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Th => "th",
        }
    }
}

pub const LANGS: [(Lang, &str); 2] = [(Lang::En, "English"), (Lang::Th, "ไทย")];

/// Fixed strings of one language.
pub struct S {
    pub net: &'static str,
    pub online: &'static str,
    pub offline: &'static str,
    pub offline_title: &'static str,
    pub no_data: &'static str,
    pub error: &'static str,
    pub not_logged_in: &'static str,
    pub all_off: &'static str,
    /// Under the empty ring on a card with nothing to show (`not_logged_in` / `all_off` are its tooltips).
    pub add_account: &'static str,
    pub pick_accounts: &'static str,
    pub more: &'static str,
    pub billing: &'static str,
    pub billing_short: &'static str,
    pub other_apps: &'static str,
    pub close: &'static str,
    pub resets_in: &'static str,
    pub today: &'static str,
    pub tomorrow: &'static str,
    pub credits_left: &'static str,
    pub extra_usage: &'static str,
    pub no_quota: &'static str,
    pub retry_hint: &'static str,
    pub u: U,
    pub menu: Menu,
    pub s: Set,
}

pub struct U {
    pub available: &'static str,
    pub update: &'static str,
    pub download: &'static str,
    pub skip: &'static str,
    pub later: &'static str,
    pub restart_note: &'static str,
    pub failed: &'static str,
    pub retry: &'static str,
    pub section: &'static str,
    pub check: &'static str,
    pub checking: &'static str,
    pub manual_hint: &'static str,
    pub check_failed: &'static str,
    pub dev_only: &'static str,
}

pub struct Menu {
    pub settings: &'static str,
    pub quit: &'static str,
    pub not_logged_in: &'static str,
}

pub struct Set {
    pub title: &'static str,
    pub general: &'static str,
    pub show_overlay: &'static str,
    pub show_overlay_hint: &'static str,
    pub language: &'static str,
    pub layout: &'static str,
    pub horizontal: &'static str,
    pub vertical: &'static str,
    pub size: &'static str,
    pub small: &'static str,
    pub medium: &'static str,
    pub large: &'static str,
    pub opacity: &'static str,
    pub startup: &'static str,
    pub startup_hint: &'static str,
    pub startup_blocked: &'static str,
    pub network_show: &'static str,
    pub network_hint: &'static str,
    pub services: &'static str,
    pub services_hint: &'static str,
    pub connected: &'static str,
    pub absent: &'static str,
    pub failed: &'static str,
    /// Old numbers kept while the server asks to wait ("too many requests").
    pub waiting: &'static str,
    pub disabled: &'static str,
    pub checking: &'static str,
    pub windows_pending: &'static str,
    pub windows_off: &'static str,
    /// In place of the accounts while there are none.
    pub no_accounts: &'static str,
    pub no_accounts_hint: &'static str,
    pub sign_in: &'static str,
    pub sign_out: &'static str,
    /// The sign-out button after one click: a second click signs out.
    pub sure: &'static str,
    pub cancel: &'static str,
    pub copy_link: &'static str,
    pub copy_code: &'static str,
    pub copied: &'static str,
    /// GitHub's device sign-in: before the code comes, and under it.
    pub getting_code: &'static str,
    pub code_hint: &'static str,
    /// Under the "finish in your browser" line.
    pub other_account: &'static str,
    pub refresh: &'static str,
    pub snap: &'static str,
    pub restart: &'static str,
    pub quit: &'static str,
    pub shortcut: &'static str,
}

pub static EN: S = S {
    net: "Net",
    online: "Connected to the internet",
    offline: "Offline",
    offline_title: "No internet connection",
    no_data: "no data",
    error: "error",
    not_logged_in: "Not logged in — click to sign in",
    all_off: "Nothing to show — click to pick accounts",
    add_account: "Sign in",
    pick_accounts: "Pick",
    more: "Settings",
    billing: "Billing cycle",
    billing_short: "Bill",
    other_apps: "Other apps",
    close: "Close",
    resets_in: "Resets in",
    today: "Today",
    tomorrow: "Tomorrow",
    credits_left: "Credits left",
    extra_usage: "Extra usage",
    no_quota: "No quota data",
    retry_hint: "Double-click the card to try again",
    u: U {
        available: "Update available",
        update: "Update",
        download: "Download",
        skip: "Skip this version",
        later: "Later",
        restart_note: "The app restarts by itself in a moment",
        failed: "Update failed",
        retry: "Try again",
        section: "Updates",
        check: "Check now",
        checking: "Checking for updates…",
        manual_hint: "Opens the download page — macOS builds update by hand",
        check_failed: "Couldn't check for updates",
        dev_only: "Updates work in the installed app",
    },
    menu: Menu { settings: "Open Ai Quota Overlay", quit: "Quit", not_logged_in: "Not logged in to Claude / Codex / Cursor" },
    s: Set {
        title: "Settings",
        general: "General",
        show_overlay: "Show overlay",
        show_overlay_hint: "Hide it for a while without quitting",
        language: "Language",
        layout: "Layout",
        horizontal: "Horizontal",
        vertical: "Vertical",
        size: "Size",
        small: "Small",
        medium: "Medium",
        large: "Large",
        opacity: "Opacity",
        startup: "Launch at startup",
        startup_hint: "Open the overlay when you sign in",
        startup_blocked: "Couldn't change it: Windows or an antivirus kept the startup setting as it was",
        network_show: "Show internet status",
        network_hint: "Pings every 3 seconds and shows the latency on the card",
        services: "Services",
        services_hint: "Sign in to as many accounts as you like, then pick which quota windows show on the card",
        connected: "Connected",
        absent: "Not logged in",
        failed: "Error",
        waiting: "Waiting to check again",
        disabled: "Off",
        checking: "Checking…",
        windows_pending: "Quota windows show up here after the first check",
        windows_off: "Turn on to check this service",
        no_accounts: "No accounts yet",
        no_accounts_hint: "Sign in with a button above — it opens in your browser. A service can have more than one account.",
        sign_in: "Sign in",
        sign_out: "Sign out",
        sure: "Sign out?",
        cancel: "Cancel",
        copy_link: "Copy link",
        copy_code: "Copy code",
        copied: "Copied",
        getting_code: "Getting a sign-in code from GitHub…",
        code_hint: "It's on your clipboard already: paste it on the GitHub page and authorize.",
        other_account: "Signed in there with another account? Sign out of it first, or open the link in a private window.",
        refresh: "Refresh now",
        snap: "Snap to corner",
        restart: "Restart",
        quit: "Quit",
        shortcut: "Show / hide",
    },
};

pub static TH: S = S {
    net: "เน็ต",
    online: "เชื่อมต่ออินเทอร์เน็ตได้",
    offline: "ออฟไลน์",
    offline_title: "เชื่อมต่ออินเทอร์เน็ตไม่ได้",
    no_data: "ไม่มีข้อมูล",
    error: "ผิดพลาด",
    not_logged_in: "ยังไม่ได้ login — คลิกเพื่อ login",
    all_off: "ไม่มีอะไรให้แสดง — คลิกเพื่อเลือกบัญชี",
    add_account: "login",
    pick_accounts: "เลือก",
    more: "ตั้งค่า",
    billing: "รอบบิล",
    billing_short: "บิล",
    other_apps: "แอปอื่น",
    close: "ปิด",
    resets_in: "รีเซ็ตใน",
    today: "วันนี้",
    tomorrow: "พรุ่งนี้",
    credits_left: "เครดิตเหลือ",
    extra_usage: "ใช้เกินแพ็กเกจ",
    no_quota: "ไม่มีข้อมูลโควต้า",
    retry_hint: "ดับเบิลคลิกที่กล่องเพื่อลองใหม่",
    u: U {
        available: "มีอัปเดตใหม่",
        update: "อัปเดต",
        download: "ดาวน์โหลด",
        skip: "ข้ามเวอร์ชันนี้",
        later: "ไว้ทีหลัง",
        restart_note: "แอปจะรีสตาร์ทเองในอีกสักครู่",
        failed: "อัปเดตไม่สำเร็จ",
        retry: "ลองใหม่",
        section: "อัปเดต",
        check: "เช็คตอนนี้",
        checking: "กำลังเช็คอัปเดต…",
        manual_hint: "เปิดหน้าดาวน์โหลด — บน macOS ต้องอัปเดตเอง",
        check_failed: "เช็คอัปเดตไม่สำเร็จ",
        dev_only: "อัปเดตใช้ได้ในแอปที่ติดตั้งแล้ว",
    },
    menu: Menu { settings: "เปิด Ai Quota Overlay", quit: "ปิดแอป", not_logged_in: "ยังไม่ได้ login Claude / Codex / Cursor" },
    s: Set {
        title: "ตั้งค่า",
        general: "ทั่วไป",
        show_overlay: "แสดงกล่องโควต้า",
        show_overlay_hint: "ซ่อนไว้ชั่วคราวโดยไม่ต้องปิดแอป",
        language: "ภาษา",
        layout: "การวาง",
        horizontal: "แนวนอน",
        vertical: "แนวตั้ง",
        size: "ขนาด",
        small: "เล็ก",
        medium: "กลาง",
        large: "ใหญ่",
        opacity: "ความทึบ",
        startup: "เปิดพร้อมเครื่อง",
        startup_hint: "เปิด overlay เองตอนเข้าเครื่อง",
        startup_blocked: "เปลี่ยนไม่ได้: Windows หรือโปรแกรมแอนตี้ไวรัสไม่ยอมให้แก้การเปิดพร้อมเครื่อง",
        network_show: "แสดงสถานะอินเทอร์เน็ต",
        network_hint: "เช็คทุก 3 วินาที และแสดงค่า ping บนกล่อง",
        services: "บริการ",
        services_hint: "login ได้หลายบัญชี แล้วเลือกว่ารอบโควต้าไหนจะขึ้นบนกล่อง",
        connected: "เชื่อมต่อแล้ว",
        absent: "ยังไม่ได้ login",
        failed: "ผิดพลาด",
        waiting: "รอเช็คใหม่",
        disabled: "ปิดอยู่",
        checking: "กำลังเช็ค…",
        windows_pending: "รอบโควต้าจะขึ้นตรงนี้หลังเช็คครั้งแรก",
        windows_off: "เปิดเพื่อเช็คบริการนี้",
        no_accounts: "ยังไม่มีบัญชี",
        no_accounts_hint: "กดปุ่มด้านบนเพื่อ login ผ่านเบราว์เซอร์ บริการเดียวเพิ่มได้หลายบัญชี",
        sign_in: "login",
        sign_out: "ออกจากระบบ",
        sure: "ยืนยันออก?",
        cancel: "ยกเลิก",
        copy_link: "คัดลอกลิงก์",
        copy_code: "คัดลอกโค้ด",
        copied: "คัดลอกแล้ว",
        getting_code: "กำลังขอโค้ด login จาก GitHub…",
        code_hint: "คัดลอกโค้ดไว้ให้แล้ว วางในหน้า GitHub แล้วกดอนุญาต",
        other_account: "ถ้าเบราว์เซอร์ login บัญชีอื่นค้างอยู่ ให้ออกจากระบบก่อน หรือเปิดลิงก์ในหน้าต่างส่วนตัว",
        refresh: "รีเฟรชตอนนี้",
        snap: "กลับไปมุมจอ",
        restart: "รีสตาร์ท",
        quit: "ปิดแอป",
        shortcut: "ซ่อน / แสดง",
    },
};

pub fn t(lang: Lang) -> &'static S {
    match lang {
        Lang::En => &EN,
        Lang::Th => &TH,
    }
}

// ---------- templated strings ----------

pub fn left_days(l: Lang, d: &str) -> String {
    match l {
        Lang::En => format!("{d}d"),
        Lang::Th => format!("{d} วัน"),
    }
}
pub fn left_hours(l: Lang, h: i64) -> String {
    match l {
        Lang::En => format!("{h}h"),
        Lang::Th => format!("{h} ชม"),
    }
}
pub fn left_minutes(l: Lang, m: i64) -> String {
    match l {
        Lang::En => format!("{m}m"),
        Lang::Th => format!("{m} นาที"),
    }
}
fn span_days(l: Lang, d: i64, short: bool) -> String {
    match (l, short) {
        (Lang::En, false) => format!("{d} days"),
        (Lang::En, true) => format!("{d}d"),
        (Lang::Th, false) => format!("{d} วัน"),
        (Lang::Th, true) => format!("{d}ว"),
    }
}
fn span_hours(l: Lang, h: i64, short: bool) -> String {
    match (l, short) {
        (Lang::En, false) => format!("{h} h"),
        (Lang::En, true) => format!("{h}h"),
        (Lang::Th, false) => format!("{h} ชม."),
        (Lang::Th, true) => format!("{h}ช"),
    }
}
pub fn until(l: Lang, d: i64, h: i64, m: i64) -> String {
    match l {
        Lang::En => {
            if d > 0 {
                format!("{d} d {h} h")
            } else if h > 0 {
                format!("{h} h {m} min")
            } else {
                format!("{m} min")
            }
        }
        Lang::Th => {
            if d > 0 {
                format!("{d} วัน {h} ชม.")
            } else if h > 0 {
                format!("{h} ชม. {m} นาที")
            } else {
                format!("{m} นาที")
            }
        }
    }
}
pub fn left_used(l: Lang, p: i64) -> String {
    match l {
        Lang::En => format!("left · {p}% used"),
        Lang::Th => format!("เหลือ · ใช้ไป {p}%"),
    }
}
pub fn from_to(a: &str, b: &str) -> String {
    format!("v{a} → v{b}")
}
pub fn downloading(l: Lang, v: &str) -> String {
    match l {
        Lang::En => format!("Downloading v{v}"),
        Lang::Th => format!("กำลังดาวน์โหลด v{v}"),
    }
}
pub fn installing(l: Lang, v: &str) -> String {
    match l {
        Lang::En => format!("Installing v{v}"),
        Lang::Th => format!("กำลังติดตั้ง v{v}"),
    }
}
pub fn up_to_date(l: Lang, v: &str) -> String {
    match l {
        Lang::En => format!("v{v} — you're up to date"),
        Lang::Th => format!("v{v} — เป็นเวอร์ชันล่าสุดแล้ว"),
    }
}
pub fn ready(l: Lang, v: &str) -> String {
    match l {
        Lang::En => format!("v{v} is available"),
        Lang::Th => format!("มี v{v} ให้อัปเดต"),
    }
}
pub fn skipped(l: Lang, v: &str) -> String {
    match l {
        Lang::En => format!("v{v} is available (skipped)"),
        Lang::Th => format!("มี v{v} (ข้ามไว้)"),
    }
}
pub fn current(l: Lang, v: &str) -> String {
    match l {
        Lang::En => format!("You're on v{v}"),
        Lang::Th => format!("เวอร์ชันปัจจุบัน v{v}"),
    }
}
/// Old numbers shown because the latest check failed.
pub fn as_of(l: Lang, when: &str) -> String {
    match l {
        Lang::En => format!("Numbers from {when}"),
        Lang::Th => format!("ตัวเลขเมื่อ {when}"),
    }
}
/// The next automatic try after "too many requests"; manual = a double-click may try now.
pub fn retry_at(l: Lang, time: &str, manual: bool) -> String {
    match (l, manual) {
        (Lang::En, true) => format!("Tries again by itself at {time} · double-click the card to try now"),
        (Lang::En, false) => format!("The server asked to wait · tries again by itself at {time}"),
        (Lang::Th, true) => format!("จะลองใหม่เองตอน {time} · ดับเบิลคลิกที่กล่องเพื่อลองตอนนี้"),
        (Lang::Th, false) => format!("เซิร์ฟเวอร์ขอให้รอ · จะลองใหม่เองตอน {time}"),
    }
}
// ---------- signing in ----------

pub fn enter_code(l: Lang, code: &str) -> String {
    match l {
        Lang::En => format!("Enter {code} on the GitHub page"),
        Lang::Th => format!("ใส่โค้ด {code} ในหน้า GitHub"),
    }
}
pub fn waiting(l: Lang, svc: &str) -> String {
    match l {
        Lang::En => format!("Finish signing in to {svc} in your browser…"),
        Lang::Th => format!("login {svc} ให้เสร็จในเบราว์เซอร์…"),
    }
}
pub fn login_failed(l: Lang, msg: &str) -> String {
    match l {
        Lang::En => format!("Couldn't sign in — {msg}"),
        Lang::Th => format!("login ไม่สำเร็จ — {msg}"),
    }
}
pub fn signed_in(l: Lang, svc: &str, who: &str) -> String {
    match (l, who.is_empty()) {
        (Lang::En, false) => format!("Signed in to {svc} as {who}"),
        (Lang::En, true) => format!("Signed in to {svc}"),
        (Lang::Th, false) => format!("login {svc} แล้ว: {who}"),
        (Lang::Th, true) => format!("login {svc} แล้ว"),
    }
}
pub fn login_timeout(l: Lang) -> &'static str {
    match l {
        Lang::En => "nothing came back from the browser in 10 minutes",
        Lang::Th => "เบราว์เซอร์ไม่ตอบกลับภายใน 10 นาที",
    }
}
pub fn login_state(l: Lang) -> &'static str {
    match l {
        Lang::En => "the browser came back from a different sign-in, try again",
        Lang::Th => "เบราว์เซอร์ส่งกลับมาจากการ login อื่น ลองใหม่อีกครั้ง",
    }
}
pub fn login_port_busy(l: Lang, port: u16) -> String {
    match l {
        Lang::En => format!("port {port} is in use — close any \"codex login\" still running and try again"),
        Lang::Th => format!("พอร์ต {port} ถูกใช้อยู่ — ปิด \"codex login\" ที่ค้างอยู่แล้วลองใหม่"),
    }
}
/// The browser page at the end of a sign-in: title and text.
pub fn login_page(l: Lang, s: crate::model::Svc, err: Option<&str>) -> (String, String) {
    let svc = s.name();
    match (l, err) {
        (Lang::En, None) => (format!("Signed in to {svc}"), "You can close this tab and go back to Ai Quota Overlay.".into()),
        (Lang::En, Some(e)) => (format!("Couldn't sign in to {svc}"), e.to_string()),
        (Lang::Th, None) => (format!("login {svc} แล้ว"), "ปิดแท็บนี้แล้วกลับไปที่ Ai Quota Overlay ได้เลย".into()),
        (Lang::Th, Some(e)) => (format!("login {svc} ไม่สำเร็จ"), e.to_string()),
    }
}

pub fn updated(l: Lang, time: &str) -> String {
    match l {
        Lang::En => format!("Updated {time}"),
        Lang::Th => format!("อัปเดต {time}"),
    }
}

/// "5 h", "7 days · Opus", "Billing cycle"; short = "5h", "7d", "Bill" for under a ring.
pub fn window_label(w: &Win, l: Lang, short: bool) -> String {
    let s = t(l);
    if w.billing {
        return if short { s.billing_short } else { s.billing }.to_string();
    }
    let mut parts = Vec::new();
    if let Some(span) = w.span.filter(|s| *s > 0.0) {
        let h = (span / 3600.0).round() as i64;
        parts.push(if h % 24 == 0 { span_days(l, h / 24, short) } else { span_hours(l, h, short) });
    }
    let name = match w.tag.as_deref() {
        Some("otherApps") => Some(s.other_apps.to_string()),
        Some(_) => None,
        None => w.name.clone().filter(|n| !n.is_empty()),
    };
    if let Some(n) = name {
        parts.push(n);
    }
    if parts.is_empty() {
        w.id.clone()
    } else {
        parts.join(" · ")
    }
}

pub fn error_text(e: &QErr, l: Lang) -> String {
    let status = e.status.clone().unwrap_or_default();
    let service = e.service.clone().unwrap_or_default();
    let message = e.message.clone().unwrap_or_default();
    match (l, e.code.as_str()) {
        (Lang::En, "offline") => "Can't reach the internet".into(),
        (Lang::En, "rateLimited") => "Too many requests (429), waiting a bit".into(),
        (Lang::En, "status") => format!("{service} returned {status}"),
        (Lang::En, "appExpired") => "Login expired — sign in again in Settings".into(),
        (Lang::En, "noCopilot") => "This GitHub account has no Copilot".into(),
        (Lang::En, _) => format!("Error: {message}"),
        (Lang::Th, "offline") => "ต่ออินเทอร์เน็ตไม่ได้".into(),
        (Lang::Th, "rateLimited") => "ถูกจำกัดการเช็ค (429) รอสักครู่".into(),
        (Lang::Th, "status") => format!("{service} ตอบ {status}"),
        (Lang::Th, "appExpired") => "login หมดอายุ — login ใหม่ในหน้าตั้งค่า".into(),
        (Lang::Th, "noCopilot") => "บัญชี GitHub นี้ไม่มี Copilot".into(),
        (Lang::Th, _) => format!("ผิดพลาด: {message}"),
    }
}
