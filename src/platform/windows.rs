//! Windows: native Win32. The card, panel and toast are layered popup windows presented with
//! `UpdateLayeredWindow` (per-pixel alpha, `SourceConstantAlpha` for the opacity setting); the app window is a
//! frameless resizable window painted with `SetDIBitsToDevice`. Per-Monitor-V2 DPI aware; positions are kept in
//! DIP laid out like Chromium's, so settings.json from the Electron build stays valid.
#![allow(clippy::missing_safety_doc)]
use crate::app::{App, Btn3, CursorKind, Display, Ev, Fetcher, Host, Key, Mouse, RectD, WinKind};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tiny_skia::Pixmap;
use windows::core::{w, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    COLORREF, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint, EnumDisplayMonitors, GetDC, GetMonitorInfoW,
    InvalidateRect, ReleaseDC, SelectObject, SetDIBitsToDevice, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HMONITOR, MONITORINFO, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, MARGINS, TTTOOLINFOW, TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS,
    TTM_ADDTOOLW, TTM_POP, TTM_SETMAXTIPWIDTH, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, ReleaseCapture, SetCapture, TrackMouseEvent, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE,
};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The uninstaller next to an installed exe (electron-builder's name, kept so both find each other).
pub const UNINSTALLER: &str = "Uninstall Ai Quota Overlay.exe";
const MAIN_CLASS: PCWSTR = w!("AiQuotaOverlay.Main");
const LAYER_CLASS: PCWSTR = w!("AiQuotaOverlay.Layer");
const APP_CLASS: PCWSTR = w!("AiQuotaOverlay.App");
const MUTEX: PCWSTR = w!("Local\\AiQuotaOverlay.SingleInstance");
const WM_WAKE: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const WM_SECOND: u32 = WM_APP + 3;
const TIMER_ID: usize = 1;
const HOTKEY_ID: i32 = 1;
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("Ai Quota Overlay");

fn idx(k: WinKind) -> usize {
    match k {
        WinKind::Card => 0,
        WinKind::Panel => 1,
        WinKind::Toast => 2,
        WinKind::App => 3,
    }
}
const KINDS: [WinKind; 4] = [WinKind::Card, WinKind::Panel, WinKind::Toast, WinKind::App];

thread_local! {
    static CTL: RefCell<Option<(App, WinHost)>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Ev>> = const { RefCell::new(VecDeque::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static MAIN: Cell<isize> = const { Cell::new(0) };
    static HWNDS: [Cell<isize>; 4] = const { [Cell::new(0), Cell::new(0), Cell::new(0), Cell::new(0)] };
    /// Device px per DIP of each window as last presented.
    static SCALES: [Cell<f64>; 4] = const { [Cell::new(1.0), Cell::new(1.0), Cell::new(1.0), Cell::new(1.0)] };
    static VISIBLE: [Cell<bool>; 4] = const { [Cell::new(false), Cell::new(false), Cell::new(false), Cell::new(false)] };
    static TRACKING: [Cell<bool>; 4] = const { [Cell::new(false), Cell::new(false), Cell::new(false), Cell::new(false)] };
    static CURSOR: Cell<CursorKind> = const { Cell::new(CursorKind::Default) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static APP_PIXELS: RefCell<(u32, u32, Vec<u8>)> = const { RefCell::new((0, 0, Vec::new())) };
    static DISPLAYS: RefCell<Vec<Mon>> = const { RefCell::new(Vec::new()) };
}

fn hwnd(k: WinKind) -> Option<HWND> {
    let v = HWNDS.with(|h| h[idx(k)].get());
    (v != 0).then_some(HWND(v as *mut _))
}

fn main_hwnd() -> HWND {
    HWND(MAIN.with(|m| m.get()) as *mut _)
}

fn kind_of(h: HWND) -> Option<WinKind> {
    let v = h.0 as isize;
    KINDS.into_iter().find(|k| HWNDS.with(|w| w[idx(*k)].get()) == v)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------- displays in DIP, the way Chromium lays them out ----------

#[derive(Clone, Copy, Debug)]
struct Mon {
    phys: RECT,
    work: RECT,
    scale: f64,
    primary: bool,
    /// DIP origin.
    dx: f64,
    dy: f64,
}

impl Mon {
    fn dip(&self, r: RECT) -> RectD {
        RectD::new(
            self.dx + (r.left - self.phys.left) as f64 / self.scale,
            self.dy + (r.top - self.phys.top) as f64 / self.scale,
            (r.right - r.left) as f64 / self.scale,
            (r.bottom - r.top) as f64 / self.scale,
        )
    }
    fn dip_bounds(&self) -> RectD {
        self.dip(self.phys)
    }
}

fn enum_monitors() -> Vec<Mon> {
    unsafe extern "system" fn cb(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let v = &mut *(data.0 as *mut Vec<Mon>);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(m, &mut info).as_bool() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            v.push(Mon { phys: info.rcMonitor, work: info.rcWork, scale: dx as f64 / 96.0, primary: info.dwFlags & 1 != 0, dx: 0.0, dy: 0.0 });
        }
        BOOL(1)
    }
    let mut v: Vec<Mon> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    layout_dip(&mut v);
    v
}

/// Primary at its physical origin scaled; every other display placed against a touching, already placed one
/// (keeping the shared edge and scaling the offset along it), as Chromium's ScreenWin does.
fn layout_dip(v: &mut [Mon]) {
    if v.is_empty() {
        return;
    }
    let p = v.iter().position(|m| m.primary).unwrap_or(0);
    v[p].dx = v[p].phys.left as f64 / v[p].scale;
    v[p].dy = v[p].phys.top as f64 / v[p].scale;
    let mut placed = vec![false; v.len()];
    placed[p] = true;
    for _ in 0..v.len() {
        for i in 0..v.len() {
            if placed[i] {
                continue;
            }
            let c = v[i];
            let parent = (0..v.len()).find(|&j| placed[j] && touches(&v[j].phys, &c.phys));
            let Some(j) = parent else { continue };
            let pm = v[j];
            let pb = pm.dip_bounds();
            let (mut x, mut y);
            let cw = (c.phys.right - c.phys.left) as f64 / c.scale;
            let ch = (c.phys.bottom - c.phys.top) as f64 / c.scale;
            if c.phys.left == pm.phys.right || c.phys.right == pm.phys.left {
                x = if c.phys.left == pm.phys.right { pb.x + pb.w } else { pb.x - cw };
                y = pb.y + (c.phys.top - pm.phys.top) as f64 / pm.scale;
            } else {
                y = if c.phys.top == pm.phys.bottom { pb.y + pb.h } else { pb.y - ch };
                x = pb.x + (c.phys.left - pm.phys.left) as f64 / pm.scale;
            }
            if !x.is_finite() || !y.is_finite() {
                x = c.phys.left as f64 / c.scale;
                y = c.phys.top as f64 / c.scale;
            }
            v[i].dx = x;
            v[i].dy = y;
            placed[i] = true;
        }
    }
    for i in 0..v.len() {
        if !placed[i] {
            v[i].dx = v[i].phys.left as f64 / v[i].scale;
            v[i].dy = v[i].phys.top as f64 / v[i].scale;
        }
    }
}

fn touches(a: &RECT, b: &RECT) -> bool {
    let vert = (a.right == b.left || a.left == b.right) && a.top < b.bottom && b.top < a.bottom;
    let horiz = (a.bottom == b.top || a.top == b.bottom) && a.left < b.right && b.left < a.right;
    vert || horiz
}

fn mons() -> Vec<Mon> {
    DISPLAYS.with(|d| {
        if d.borrow().is_empty() {
            *d.borrow_mut() = enum_monitors();
        }
        d.borrow().clone()
    })
}

fn refresh_monitors() {
    let m = enum_monitors();
    DISPLAYS.with(|d| *d.borrow_mut() = m);
}

/// The monitor whose DIP bounds hold the most of `r` (else the nearest).
fn mon_for(r: RectD) -> Mon {
    let ms = mons();
    let area = |m: &Mon| {
        let b = m.dip_bounds();
        let w = (r.x + r.w).min(b.x + b.w) - r.x.max(b.x);
        let h = (r.y + r.h).min(b.y + b.h) - r.y.max(b.y);
        if w > 0.0 && h > 0.0 {
            w * h
        } else {
            0.0
        }
    };
    let best = ms.iter().cloned().max_by(|a, b| area(a).total_cmp(&area(b)));
    if let Some(b) = best.filter(|b| area(b) > 0.0) {
        return b;
    }
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let dist = |m: &Mon| {
        let b = m.dip_bounds();
        let dx = (b.x - cx).max(0.0).max(cx - (b.x + b.w));
        let dy = (b.y - cy).max(0.0).max(cy - (b.y + b.h));
        dx * dx + dy * dy
    };
    ms.into_iter().min_by(|a, b| dist(a).total_cmp(&dist(b))).unwrap_or(Mon {
        phys: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
        work: RECT { left: 0, top: 0, right: 1920, bottom: 1040 },
        scale: 1.0,
        primary: true,
        dx: 0.0,
        dy: 0.0,
    })
}

/// DIP rect → physical rect (origin through its monitor, size scaled by that monitor).
fn to_phys(r: RectD) -> (RECT, f64) {
    let m = mon_for(r);
    let x = m.phys.left + ((r.x - m.dx) * m.scale).round() as i32;
    let y = m.phys.top + ((r.y - m.dy) * m.scale).round() as i32;
    let w = (r.w * m.scale).round().max(1.0) as i32;
    let h = (r.h * m.scale).round().max(1.0) as i32;
    (RECT { left: x, top: y, right: x + w, bottom: y + h }, m.scale)
}

fn phys_to_dip(x: i32, y: i32) -> (f64, f64) {
    let ms = mons();
    let m = ms
        .iter()
        .find(|m| x >= m.phys.left && x < m.phys.right && y >= m.phys.top && y < m.phys.bottom)
        .cloned()
        .or_else(|| ms.first().cloned());
    match m {
        Some(m) => (m.dx + (x - m.phys.left) as f64 / m.scale, m.dy + (y - m.phys.top) as f64 / m.scale),
        None => (x as f64, y as f64),
    }
}

// ---------- the host ----------

pub struct WinHost {
    tray_added: bool,
    tray_tip: String,
    tooltips: [isize; 3],
    tips: [Option<String>; 3],
    icon_big: HICON,
    icon_small: HICON,
    mutex: HANDLE,
}

impl WinHost {
    fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut d = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: main_hwnd(),
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon_small,
            ..Default::default()
        };
        let tip: Vec<u16> = self.tray_tip.encode_utf16().take(127).collect();
        d.szTip[..tip.len()].copy_from_slice(&tip);
        d
    }

    fn add_tray(&mut self) {
        let d = self.tray_data();
        unsafe {
            self.tray_added = Shell_NotifyIconW(NIM_ADD, &d).as_bool();
        }
    }

    fn create_layer(kind: WinKind) -> HWND {
        let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        // Only the open detail panel takes focus, so clicking anywhere else dismisses it.
        if kind != WinKind::Panel {
            ex |= WS_EX_NOACTIVATE;
        }
        unsafe {
            let h = CreateWindowExW(ex, LAYER_CLASS, w!("Ai Quota Overlay"), WS_POPUP, 0, 0, 1, 1, None, None, None, None).unwrap_or_default();
            HWNDS.with(|w| w[idx(kind)].set(h.0 as isize));
            h
        }
    }

    fn tooltip_for(&mut self, k: WinKind) -> Option<HWND> {
        let i = match k {
            WinKind::Card => 0,
            WinKind::Panel => 1,
            WinKind::Toast => 2,
            WinKind::App => return None,
        };
        if self.tooltips[i] == 0 {
            let owner = hwnd(k)?;
            unsafe {
                let tt = CreateWindowExW(
                    WS_EX_TOPMOST,
                    TOOLTIPS_CLASSW,
                    None,
                    WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX),
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    Some(owner),
                    None,
                    None,
                    None,
                )
                .ok()?;
                let mut empty = wide("");
                let ti = TTTOOLINFOW {
                    cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                    hwnd: owner,
                    uId: owner.0 as usize,
                    lpszText: PWSTR(empty.as_mut_ptr()),
                    ..Default::default()
                };
                SendMessageW(tt, TTM_ADDTOOLW, None, Some(LPARAM(&ti as *const _ as isize)));
                SendMessageW(tt, TTM_SETMAXTIPWIDTH, None, Some(LPARAM(400)));
                self.tooltips[i] = tt.0 as isize;
            }
        }
        Some(HWND(self.tooltips[i] as *mut _))
    }
}

impl Host for WinHost {
    fn displays(&self) -> Vec<Display> {
        mons()
            .iter()
            .map(|m| Display { bounds: m.dip_bounds(), work: m.dip(m.work), scale: m.scale })
            .collect()
    }

    fn primary(&self) -> Display {
        let ms = mons();
        let m = ms.iter().find(|m| m.primary).or(ms.first()).cloned().unwrap_or_else(|| mon_for(RectD::new(0.0, 0.0, 1.0, 1.0)));
        Display { bounds: m.dip_bounds(), work: m.dip(m.work), scale: m.scale }
    }

    fn cursor(&self) -> (f64, f64) {
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        phys_to_dip(p.x, p.y)
    }

    fn device_size(&self, _w: WinKind, r: RectD) -> (f64, u32, u32) {
        let (p, s) = to_phys(r);
        (s, (p.right - p.left) as u32, (p.bottom - p.top) as u32)
    }

    fn present(&mut self, k: WinKind, r: RectD, pm: &Pixmap, alpha: f64, activate: bool) {
        let h = match hwnd(k) {
            Some(h) => h,
            None => Self::create_layer(k),
        };
        let (p, scale) = to_phys(r);
        SCALES.with(|s| s[idx(k)].set(scale));
        unsafe {
            update_layered(h, p.left, p.top, pm, alpha);
            let was = VISIBLE.with(|v| v[idx(k)].replace(true));
            if !was {
                if activate {
                    let _ = ShowWindow(h, SW_SHOW);
                    let _ = SetForegroundWindow(h);
                } else {
                    let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
                }
                let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            } else if activate {
                let _ = SetForegroundWindow(h);
            }
        }
    }

    fn hide(&mut self, k: WinKind) {
        if let Some(h) = hwnd(k) {
            VISIBLE.with(|v| v[idx(k)].set(false));
            unsafe {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }

    fn set_cursor(&mut self, c: CursorKind) {
        if CURSOR.with(|x| x.replace(c)) != c {
            unsafe {
                let _ = SetCursor(Some(load_cursor(c)));
            }
        }
    }

    fn set_tooltip(&mut self, k: WinKind, text: Option<&str>) {
        let i = match k {
            WinKind::Card => 0,
            WinKind::Panel => 1,
            WinKind::Toast => 2,
            WinKind::App => return,
        };
        if self.tips[i].as_deref() == text {
            return;
        }
        self.tips[i] = text.map(String::from);
        let Some(owner) = hwnd(k) else { return };
        let Some(tt) = self.tooltip_for(k) else { return };
        let mut t = wide(text.unwrap_or(""));
        let ti = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: owner,
            uId: owner.0 as usize,
            lpszText: PWSTR(t.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            if text.is_none() {
                SendMessageW(tt, TTM_POP, None, None);
            }
            SendMessageW(tt, TTM_UPDATETIPTEXTW, None, Some(LPARAM(&ti as *const _ as isize)));
        }
    }

    fn app_open(&mut self, r: RectD, title: &str) {
        unsafe {
            if let Some(h) = hwnd(WinKind::App) {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = ShowWindow(h, SW_SHOW);
                let _ = SetForegroundWindow(h);
                return;
            }
            let (p, scale) = to_phys(r);
            let t = wide(title);
            let style = WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_SYSMENU | WS_CLIPCHILDREN;
            let Ok(h) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                APP_CLASS,
                PCWSTR(t.as_ptr()),
                style,
                p.left,
                p.top,
                p.right - p.left,
                p.bottom - p.top,
                None,
                None,
                None,
                None,
            ) else {
                return;
            };
            HWNDS.with(|w| w[idx(WinKind::App)].set(h.0 as isize));
            SCALES.with(|s| s[idx(WinKind::App)].set(scale));
            // A 1 px frame extension gives the frameless window the DWM shadow (and rounded corners on Windows 11).
            let m = MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 1, cyBottomHeight: 0 };
            let _ = DwmExtendFrameIntoClientArea(h, &m);
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(self.icon_big.0 as isize)));
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(self.icon_small.0 as isize)));
            let _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER);
            VISIBLE.with(|v| v[idx(WinKind::App)].set(true));
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
    }

    fn app_close(&mut self) {
        if let Some(h) = hwnd(WinKind::App) {
            HWNDS.with(|w| w[idx(WinKind::App)].set(0));
            VISIBLE.with(|v| v[idx(WinKind::App)].set(false));
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
    }

    fn app_is_open(&self) -> bool {
        hwnd(WinKind::App).is_some()
    }

    fn app_size(&self) -> Option<(f64, f64, f64)> {
        let h = hwnd(WinKind::App)?;
        let mut r = RECT::default();
        unsafe {
            GetClientRect(h, &mut r).ok()?;
            let s = GetDpiForWindow(h) as f64 / 96.0;
            let s = if s > 0.0 { s } else { 1.0 };
            SCALES.with(|x| x[idx(WinKind::App)].set(s));
            Some(((r.right - r.left) as f64 / s, (r.bottom - r.top) as f64 / s, s))
        }
    }

    fn app_present(&mut self, pm: &Pixmap) {
        let Some(h) = hwnd(WinKind::App) else { return };
        // RGBA (opaque) → BGRA rows for SetDIBitsToDevice.
        let mut bgra = pm.data().to_vec();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        APP_PIXELS.with(|p| *p.borrow_mut() = (pm.width(), pm.height(), bgra));
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }

    fn app_title(&mut self, title: &str) {
        if let Some(h) = hwnd(WinKind::App) {
            let t = wide(title);
            unsafe {
                let _ = SetWindowTextW(h, PCWSTR(t.as_ptr()));
            }
        }
    }

    fn app_drag(&mut self) {
        // Windows drags through WM_NCHITTEST → HTCAPTION over the title bar.
    }

    fn tray_tooltip(&mut self, text: &str) {
        if self.tray_tip == text {
            return;
        }
        self.tray_tip = text.to_string();
        if self.tray_added {
            let d = self.tray_data();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
            }
        }
    }

    fn tray_menu(&mut self, _open: &str, _quit: &str) {
        // Windows trays have click events; left or right click opens the app window.
    }

    fn login_item(&self) -> Option<bool> {
        Some(run_entries().iter().any(|(_, cmd)| points_to_exe(cmd)))
    }

    fn set_login_item(&mut self, on: bool) {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        unsafe {
            let mut key = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE | KEY_READ, &mut key).is_err() {
                return;
            }
            // Electron wrote its own value name; drop every entry that starts this exe, then add ours if wanted.
            for (name, cmd) in run_entries() {
                if points_to_exe(&cmd) {
                    let n = wide(&name);
                    let _ = RegDeleteValueW(key, PCWSTR(n.as_ptr()));
                }
            }
            if on {
                let cmd = format!("\"{exe}\"");
                let data: Vec<u8> = wide(&cmd).iter().flat_map(|u| u.to_le_bytes()).collect();
                let _ = RegSetValueExW(key, RUN_NAME, None, REG_SZ, Some(&data));
            }
            let _ = RegCloseKey(key);
        }
    }

    fn open_url(&mut self, url: &str) {
        let u = wide(url);
        unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }

    fn quit(&mut self) {
        unsafe {
            PostQuitMessage(0);
        }
    }

    fn relaunch(&mut self) {
        if let Ok(exe) = std::env::current_exe() {
            unsafe {
                let _ = ReleaseMutex(self.mutex);
            }
            let _ = std::process::Command::new(exe).arg("--relaunch").spawn();
        }
        self.quit();
    }

    fn set_timer(&mut self, after: Duration) {
        unsafe {
            SetTimer(Some(main_hwnd()), TIMER_ID, after.as_millis().clamp(1, u32::MAX as u128) as u32, None);
        }
    }
}

fn run_entries() -> Vec<(String, String)> {
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0..512u32 {
            let mut name = [0u16; 512];
            let mut nlen = name.len() as u32;
            let mut data = vec![0u8; 4096];
            let mut dlen = data.len() as u32;
            let mut ty = 0u32;
            let r = RegEnumValueW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut nlen, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut dlen));
            if r.is_err() {
                break;
            }
            let n = String::from_utf16_lossy(&name[..nlen as usize]);
            let words: Vec<u16> = data[..dlen as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let v = String::from_utf16_lossy(&words).trim_end_matches('\0').to_string();
            out.push((n, v));
        }
        let _ = RegCloseKey(key);
    }
    out
}

fn points_to_exe(cmd: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    cmd.to_lowercase().contains(&exe.display().to_string().to_lowercase())
}

unsafe fn update_layered(h: HWND, x: i32, y: i32, pm: &Pixmap, alpha: f64) {
    let (w, hgt) = (pm.width() as i32, pm.height() as i32);
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -hgt,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        if !bits.is_null() {
            // tiny-skia is premultiplied RGBA; layered windows want premultiplied BGRA.
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * hgt * 4) as usize);
            for (d, s) in dst.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
        let old = SelectObject(mem, bmp.into());
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pos = POINT { x, y };
        let size = SIZE { cx: w, cy: hgt };
        let src = POINT { x: 0, y: 0 };
        let _ = UpdateLayeredWindow(h, Some(screen), Some(&pos), Some(&size), Some(mem), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
    }
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
}

fn load_cursor(c: CursorKind) -> HCURSOR {
    let id = match c {
        CursorKind::Default | CursorKind::Grab => IDC_ARROW,
        CursorKind::Pointer => IDC_HAND,
        CursorKind::Grabbing => IDC_SIZEALL,
    };
    unsafe { LoadCursorW(None, id).unwrap_or_default() }
}

/// An HICON of `size` px from the app icon.
fn make_icon(size: u32) -> HICON {
    let Some(src) = crate::ui::app_window::icon_pixmap() else { return HICON::default() };
    let Some(mut pm) = Pixmap::new(size, size) else { return HICON::default() };
    let k = size as f32 / src.width() as f32;
    let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
    pm.draw_pixmap(0, 0, src.as_ref(), &paint, tiny_skia::Transform::from_scale(k, k), None);
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else { return HICON::default() };
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
        for (d, s) in dst.chunks_exact_mut(4).zip(pm.pixels()) {
            let c = s.demultiply();
            d[0] = c.blue();
            d[1] = c.green();
            d[2] = c.red();
            d[3] = c.alpha();
        }
        let mask = windows::Win32::Graphics::Gdi::CreateBitmap(size as i32, size as i32, 1, 1, None);
        let info = ICONINFO { fIcon: BOOL(1), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

// ---------- event plumbing ----------

/// Runs `ev` through the app, or queues it when the app is busy (a Win32 call inside the app sent a message).
fn dispatch(ev: Ev) {
    QUEUE.with(|q| q.borrow_mut().push_back(ev));
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    loop {
        let Some(ev) = QUEUE.with(|q| q.borrow_mut().pop_front()) else { break };
        CTL.with(|c| {
            if let Some((app, host)) = c.borrow_mut().as_mut() {
                app.event(host, ev);
            }
        });
    }
    BUSY.with(|b| b.set(false));
}

fn lparam_xy(l: LPARAM) -> (i32, i32) {
    ((l.0 & 0xffff) as i16 as i32, ((l.0 >> 16) & 0xffff) as i16 as i32)
}

fn mouse(kind: WinKind, h: HWND, m: Mouse, l: LPARAM) {
    let (x, y) = lparam_xy(l);
    let s = SCALES.with(|s| s[idx(kind)].get());
    if !TRACKING.with(|t| t[idx(kind)].replace(true)) {
        let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
        unsafe {
            let _ = TrackMouseEvent(&mut t);
        }
    }
    dispatch(Ev::Mouse { w: kind, m, x: x as f64 / s, y: y as f64 / s });
}

unsafe extern "system" fn main_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == TIMER_ID => {
            let _ = KillTimer(Some(h), TIMER_ID);
            dispatch(Ev::Timer);
            LRESULT(0)
        }
        WM_WAKE => {
            dispatch(Ev::Wake);
            LRESULT(0)
        }
        WM_SECOND => {
            dispatch(Ev::SecondInstance);
            LRESULT(0)
        }
        WM_HOTKEY => {
            dispatch(Ev::Hotkey);
            LRESULT(0)
        }
        WM_TRAY => {
            let ev = (lp.0 & 0xffff) as u32;
            if ev == WM_LBUTTONUP || ev == WM_RBUTTONUP {
                dispatch(Ev::TrayClick);
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            // PBT_APMRESUMEAUTOMATIC / PBT_APMRESUMESUSPEND
            if wp.0 == 0x12 || wp.0 == 0x7 {
                dispatch(Ev::Resume);
            }
            LRESULT(1)
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED => {
            refresh_monitors();
            dispatch(Ev::DisplaysChanged);
            DefWindowProcW(h, msg, wp, lp)
        }
        _ => {
            if msg != 0 && msg == TASKBAR_CREATED.with(|t| t.get()) {
                // Explorer restarted: put the tray icon back.
                CTL.with(|c| {
                    if let Ok(mut c) = c.try_borrow_mut() {
                        if let Some((_, host)) = c.as_mut() {
                            host.add_tray();
                        }
                    }
                });
                return LRESULT(0);
            }
            DefWindowProcW(h, msg, wp, lp)
        }
    }
}

unsafe extern "system" fn layer_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(kind) = kind_of(h) else { return DefWindowProcW(h, msg, wp, lp) };
    match msg {
        WM_MOUSEACTIVATE => {
            if kind == WinKind::Panel {
                LRESULT(MA_ACTIVATE as isize)
            } else {
                LRESULT(MA_NOACTIVATE as isize)
            }
        }
        WM_SETCURSOR => {
            if (lp.0 & 0xffff) as u32 == HTCLIENT {
                SetCursor(Some(load_cursor(CURSOR.with(|c| c.get()))));
                return LRESULT(1);
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        WM_MOUSEMOVE => {
            mouse(kind, h, Mouse::Move, lp);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING.with(|t| t[idx(kind)].set(false));
            dispatch(Ev::Mouse { w: kind, m: Mouse::Leave, x: -1.0, y: -1.0 });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            SetCapture(h);
            mouse(kind, h, Mouse::Down(Btn3::Left), lp);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            // The second press of a double click: a press, then the double click (as pointer events gave).
            SetCapture(h);
            mouse(kind, h, Mouse::Down(Btn3::Left), lp);
            mouse(kind, h, Mouse::Dbl(Btn3::Left), lp);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            mouse(kind, h, Mouse::Up(Btn3::Left), lp);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            mouse(kind, h, Mouse::Up(Btn3::Right), lp);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => LRESULT(0),
        WM_ACTIVATE => {
            if kind == WinKind::Panel && (wp.0 & 0xffff) as u32 == WA_INACTIVE {
                dispatch(Ev::Blur(WinKind::Panel));
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe extern "system" fn app_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let kind = WinKind::App;
    match msg {
        WM_NCCALCSIZE if wp.0 != 0 => {
            // No system frame: the whole window is client area (resize borders come from WM_NCHITTEST).
            if IsZoomed(h).as_bool() {
                return DefWindowProcW(h, msg, wp, lp);
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let (sx, sy) = lparam_xy(lp);
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            let s = GetDpiForWindow(h) as f64 / 96.0;
            let b = (6.0 * s).round() as i32;
            let (x, y) = (sx - r.left, sy - r.top);
            let (w, hh) = (r.right - r.left, r.bottom - r.top);
            let (l, t, rt, bt) = (x < b, y < b, x >= w - b, y >= hh - b);
            let hit = match (l, t, rt, bt) {
                (true, true, _, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, _, true, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, _, true, _) => HTRIGHT,
                (_, true, _, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ => {
                    // The title bar (46 px) drags the window, except its close button (32×30, 8 px from the right).
                    let (dx, dy) = (x as f64 / s, y as f64 / s);
                    let wd = w as f64 / s;
                    let on_close = dx >= wd - 40.0 && dx < wd - 8.0 && dy >= 7.5 && dy < 37.5;
                    if dy < 46.0 && !on_close {
                        HTCAPTION
                    } else {
                        HTCLIENT
                    }
                }
            };
            LRESULT(hit as isize)
        }
        WM_NCLBUTTONDBLCLK => LRESULT(0), // not maximizable
        WM_GETMINMAXINFO => {
            let info = &mut *(lp.0 as *mut MINMAXINFO);
            let s = GetDpiForWindow(h) as f64 / 96.0;
            let s = if s > 0.0 { s } else { 1.0 };
            info.ptMinTrackSize = POINT { x: (380.0 * s) as i32, y: (420.0 * s) as i32 };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            dispatch(Ev::Resized(kind));
            LRESULT(0)
        }
        WM_SIZE => {
            dispatch(Ev::Resized(kind));
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            APP_PIXELS.with(|p| {
                let p = p.borrow();
                if p.0 > 0 {
                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: p.0 as i32,
                            biHeight: -(p.1 as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    SetDIBitsToDevice(dc, 0, 0, p.0, p.1, 0, 0, 0, p.1, p.2.as_ptr() as *const _, &bmi, DIB_RGB_COLORS);
                }
            });
            let _ = EndPaint(h, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SETCURSOR => {
            if (lp.0 & 0xffff) as u32 == HTCLIENT {
                SetCursor(Some(load_cursor(CURSOR.with(|c| c.get()))));
                return LRESULT(1);
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        WM_MOUSEMOVE => {
            mouse(kind, h, Mouse::Move, lp);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING.with(|t| t[idx(kind)].set(false));
            dispatch(Ev::Mouse { w: kind, m: Mouse::Leave, x: -1.0, y: -1.0 });
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            SetCapture(h);
            mouse(kind, h, Mouse::Down(Btn3::Left), lp);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            mouse(kind, h, Mouse::Up(Btn3::Left), lp);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
            dispatch(Ev::Mouse { w: kind, m: Mouse::Wheel(delta), x: 0.0, y: 0.0 });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = if wp.0 as u16 == VK_ESCAPE.0 { Key::Escape } else { Key::Other };
            dispatch(Ev::Key { w: kind, key });
            LRESULT(0)
        }
        WM_CLOSE => {
            dispatch(Ev::Close(kind));
            LRESULT(0)
        }
        WM_DESTROY => {
            if HWNDS.with(|w| w[idx(kind)].get()) == h.0 as isize {
                HWNDS.with(|w| w[idx(kind)].set(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe fn register(class: PCWSTR, proc_: WNDPROC, style: WNDCLASS_STYLES, icon: HICON) {
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style,
        lpfnWndProc: proc_,
        hInstance: hinst.into(),
        hIcon: icon,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassExW(&wc);
}

/// Single instance: the first copy owns the mutex. A later start tells it to show itself and exits, unless it was
/// started by Restart (`--relaunch`), which waits for the old copy to go.
fn single_instance(relaunch: bool) -> Option<HANDLE> {
    unsafe {
        let m = CreateMutexW(None, true, MUTEX).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if relaunch {
                let r = WaitForSingleObject(m, 15_000);
                if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                    return Some(m);
                }
            }
            if let Ok(other) = FindWindowW(MAIN_CLASS, None) {
                let _ = PostMessageW(Some(other), WM_SECOND, WPARAM(0), LPARAM(0));
            }
            return None;
        }
        Some(m)
    }
}

pub fn run(fetcher: Fetcher) {
    let relaunch = std::env::args().any(|a| a == "--relaunch");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let Some(mutex) = single_instance(relaunch) else { return };
    unsafe {
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_WIN95_CLASSES };
        let _ = InitCommonControlsEx(&icc);
        let big = make_icon(GetSystemMetrics(SM_CXICON).max(32) as u32);
        let small = make_icon(GetSystemMetrics(SM_CXSMICON).max(16) as u32);
        register(MAIN_CLASS, Some(main_proc), WNDCLASS_STYLES(0), big);
        register(LAYER_CLASS, Some(layer_proc), CS_DBLCLKS, big);
        register(APP_CLASS, Some(app_proc), CS_DBLCLKS, big);
        let main = CreateWindowExW(WS_EX_TOOLWINDOW, MAIN_CLASS, w!("Ai Quota Overlay"), WS_POPUP, 0, 0, 0, 0, None, None, None, None)
            .expect("main window");
        MAIN.with(|m| m.set(main.0 as isize));
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
        let _ = RegisterHotKey(Some(main), HOTKEY_ID, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'Q' as u32);

        let main_ptr = main.0 as isize;
        let waker: crate::app::Waker = Arc::new(move || {
            let _ = PostMessageW(Some(HWND(main_ptr as *mut _)), WM_WAKE, WPARAM(0), LPARAM(0));
        });
        let mut host = WinHost {
            tray_added: false,
            tray_tip: crate::settings::APP_NAME.to_string(),
            tooltips: [0; 3],
            tips: [None, None, None],
            icon_big: big,
            icon_small: small,
            mutex,
        };
        host.add_tray();
        let mut app = App::new(fetcher, waker);
        BUSY.with(|b| b.set(true));
        app.start(&mut host);
        CTL.with(|c| *c.borrow_mut() = Some((app, host)));
        BUSY.with(|b| b.set(false));
        dispatch(Ev::Wake);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        CTL.with(|c| {
            if let Some((_, host)) = c.borrow_mut().as_mut() {
                if host.tray_added {
                    let d = host.tray_data();
                    let _ = Shell_NotifyIconW(NIM_DELETE, &d);
                }
            }
        });
        let _ = ReleaseMutex(mutex);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn mon(l: i32, t: i32, r: i32, b: i32, scale: f64, primary: bool) -> Mon {
        let rc = RECT { left: l, top: t, right: r, bottom: b };
        Mon { phys: rc, work: rc, scale, primary, dx: 0.0, dy: 0.0 }
    }

    #[test]
    fn second_monitor_is_placed_against_the_primary_like_chromium() {
        // 2K at 100 % plus a 4K at 125 % to its right, tops aligned (the owner's setup).
        let mut v = vec![mon(0, 0, 2560, 1440, 1.0, true), mon(2560, 0, 6400, 2160, 1.25, false)];
        layout_dip(&mut v);
        assert_eq!((v[0].dx, v[0].dy), (0.0, 0.0));
        assert_eq!((v[1].dx, v[1].dy), (2560.0, 0.0));
        let b = v[1].dip_bounds();
        assert_eq!((b.w, b.h), (3072.0, 1728.0));
        // A 4K at 150 % on the left, its top 200 px lower.
        let mut v = vec![mon(0, 0, 2560, 1440, 1.0, true), mon(-3840, 200, 0, 2360, 1.5, false)];
        layout_dip(&mut v);
        assert_eq!((v[1].dx, v[1].dy), (-2560.0, 200.0));
    }

    #[test]
    fn dip_of_a_point_uses_its_monitor() {
        let mut v = vec![mon(0, 0, 2560, 1440, 1.0, true), mon(2560, 0, 6400, 2160, 1.25, false)];
        layout_dip(&mut v);
        let r = v[1].dip(RECT { left: 2560 + 1250, top: 500, right: 2560 + 1500, bottom: 625 });
        assert_eq!((r.x, r.y, r.w, r.h), (2560.0 + 1000.0, 400.0, 200.0, 100.0));
    }
}
