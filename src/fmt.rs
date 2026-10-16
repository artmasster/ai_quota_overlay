//! Local dates, times and numbers as the pages' `toLocale…String` calls printed them for en-US and th-TH.
use crate::i18n::{self, Lang};
use chrono::{Datelike, Local, TimeZone, Timelike};

fn local(ms: i64) -> chrono::DateTime<Local> {
    Local.timestamp_millis_opt(ms).single().unwrap_or_else(Local::now)
}

/// `toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })`: "09:01 AM" / "09:01".
pub fn time(ms: i64, lang: Lang) -> String {
    let t = local(ms);
    match lang {
        Lang::En => {
            let (pm, h) = t.hour12();
            format!("{:02}:{:02} {}", h, t.minute(), if pm { "PM" } else { "AM" })
        }
        Lang::Th => format!("{:02}:{:02}", t.hour(), t.minute()),
    }
}

const EN_WD: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const EN_MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const TH_WD: [&str; 7] = ["จ.", "อ.", "พ.", "พฤ.", "ศ.", "ส.", "อา."];
const TH_MO: [&str; 12] = ["ม.ค.", "ก.พ.", "มี.ค.", "เม.ย.", "พ.ค.", "มิ.ย.", "ก.ค.", "ส.ค.", "ก.ย.", "ต.ค.", "พ.ย.", "ธ.ค."];

/// "Today 09:01 AM", "Tomorrow …", else "Mon, Oct 19 02:29 PM" / "จ. 19 ต.ค. 14:29".
pub fn when(ms: i64, now: i64, lang: Lang) -> String {
    let t = i18n::t(lang);
    let at = local(ms);
    let today = local(now).date_naive();
    let diff = (at.date_naive() - today).num_days();
    let time = time(ms, lang);
    if diff == 0 {
        return format!("{} {time}", t.today);
    }
    if diff == 1 {
        return format!("{} {time}", t.tomorrow);
    }
    let wd = at.weekday().num_days_from_monday() as usize;
    let mo = at.month0() as usize;
    match lang {
        Lang::En => format!("{}, {} {} {time}", EN_WD[wd], EN_MO[mo], at.day()),
        Lang::Th => format!("{} {} {} {time}", TH_WD[wd], at.day(), TH_MO[mo]),
    }
}

/// Time left as "2 h 41 min" (rounded to whole seconds first, like the page).
pub fn until(ms: i64, now: i64, lang: Lang) -> String {
    let mut s = ((ms - now) as f64 / 1000.0).round().max(0.0) as i64;
    let d = s / 86400;
    s %= 86400;
    let h = s / 3600;
    s %= 3600;
    let m = s / 60;
    i18n::until(lang, d, h, m)
}

/// Whole number with thousands separators ("1,972"), as en-US and th-TH both print it.
pub fn grouped(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}
