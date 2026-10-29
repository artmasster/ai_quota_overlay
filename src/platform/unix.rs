//! macOS and Linux: winit for windows and input, pixels presented by the app itself so overlay windows keep
//! per-pixel alpha (macOS: the view's CALayer contents; Linux: PutImage into a 32-bit ARGB X11 window, so
//! Linux runs on X11 / XWayland like the Electron build did). Tray: NSStatusItem (tray-icon) on macOS,
//! StatusNotifierItem (ksni) on Linux. Ctrl+Alt+Q through global-hotkey.
use crate::app::{App, Btn3, CursorKind, Display, Ev, Fetcher, Host, Key, Mouse, RectD, WinKind};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowAttributes, WindowId, WindowLevel};

#[cfg(target_os = "macos")]
#[path = "mac.rs"]
mod os;
#[cfg(not(target_os = "macos"))]
#[path = "x11.rs"]
mod os;

#[allow(dead_code)] // the tray menu items exist on Linux only
#[derive(Debug, Clone, Copy)]
pub enum UserEv {
    Wake,
    Tray,
    TrayOpen,
    TrayQuit,
    Hotkey,
    Second,
}

const KINDS: [WinKind; 4] = [WinKind::Card, WinKind::Panel, WinKind::Toast, WinKind::App];

fn idx(k: WinKind) -> usize {
    match k {
        WinKind::Card => 0,
        WinKind::Panel => 1,
        WinKind::Toast => 2,
        WinKind::App => 3,
    }
}

pub struct Win {
    pub window: Arc<Window>,
    /// Device px per DIP as last presented.
    pub scale: f64,
    pub visible: bool,
    /// Last presented pixels (premultiplied RGBA) for re-presenting on expose.
    pub last: Option<Pixmap>,
    pub alpha: f64,
    pub cursor: (f64, f64),
    pub last_click: Option<(Instant, f64, f64)>,
    #[allow(dead_code)] // empty on macOS
    pub os: os::WinData,
}

pub struct UnixHost {
    el: *const ActiveEventLoop,
    pub wins: [Option<Win>; 4],
    timer_at: Option<Instant>,
    cursor: CursorKind,
    tray: os::Tray,
    title: String,
    quit: bool,
    relaunch: bool,
    proxy: EventLoopProxy<UserEv>,
    app_size: Option<(f64, f64)>,
}

impl UnixHost {
    fn el(&self) -> &ActiveEventLoop {
        // Only called while the event loop is inside a callback (set by `with_app`).
        unsafe { &*self.el }
    }

    fn create(&mut self, kind: WinKind, r: RectD, title: &str) -> Option<&mut Win> {
        let mut attrs = WindowAttributes::default()
            .with_title(title)
            .with_decorations(false)
            .with_visible(false)
            .with_active(kind == WinKind::App || kind == WinKind::Panel);
        if kind == WinKind::App {
            attrs = attrs
                .with_resizable(false)
                .with_inner_size(os::size_for(self.el(), r))
                .with_position(os::pos_for(self.el(), r));
        } else {
            attrs = attrs
                .with_transparent(true)
                .with_resizable(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_inner_size(os::size_for(self.el(), r))
                .with_position(os::pos_for(self.el(), r));
        }
        attrs = os::window_attrs(attrs, kind);
        let window = Arc::new(self.el().create_window(attrs).ok()?);
        let data = os::WinData::new(&window, kind);
        self.wins[idx(kind)] = Some(Win {
            window,
            scale: 1.0,
            visible: false,
            last: None,
            alpha: 1.0,
            cursor: (0.0, 0.0),
            last_click: None,
            os: data,
        });
        self.wins[idx(kind)].as_mut()
    }

    fn kind_of(&self, id: WindowId) -> Option<WinKind> {
        KINDS.into_iter().find(|k| self.wins[idx(*k)].as_ref().is_some_and(|w| w.window.id() == id))
    }
}

impl Host for UnixHost {
    fn displays(&self) -> Vec<Display> {
        os::displays(self.el())
    }

    fn primary(&self) -> Display {
        let ds = self.displays();
        ds.first().cloned().unwrap_or(Display {
            bounds: RectD::new(0.0, 0.0, 1920.0, 1080.0),
            work: RectD::new(0.0, 0.0, 1920.0, 1040.0),
            scale: 1.0,
        })
    }

    fn cursor(&self) -> (f64, f64) {
        os::cursor(self.el())
    }

    fn device_size(&self, _w: WinKind, r: RectD) -> (f64, u32, u32) {
        let s = os::scale_for(self.el(), r);
        (s, (r.w * s).round().max(1.0) as u32, (r.h * s).round().max(1.0) as u32)
    }

    fn present(&mut self, k: WinKind, r: RectD, pm: &Pixmap, alpha: f64, activate: bool) {
        if self.wins[idx(k)].is_none() && self.create(k, r, "Ai Quota Overlay").is_none() {
            return;
        }
        let scale = os::scale_for(self.el(), r);
        let w = self.wins[idx(k)].as_mut().unwrap();
        w.scale = scale;
        w.alpha = alpha;
        os::place(&w.window, r, pm.width(), pm.height());
        w.last = Some(pm.clone());
        if !w.visible {
            w.visible = true;
            w.window.set_visible(true);
            os::after_show(&w.window, k);
        }
        os::present(w, pm, alpha);
        if activate {
            w.window.focus_window();
        }
    }

    fn hide(&mut self, k: WinKind) {
        if let Some(w) = self.wins[idx(k)].as_mut() {
            w.visible = false;
            w.window.set_visible(false);
        }
    }

    fn set_cursor(&mut self, c: CursorKind) {
        if c == self.cursor {
            return;
        }
        self.cursor = c;
        let icon = match c {
            CursorKind::Default => CursorIcon::Default,
            CursorKind::Pointer => CursorIcon::Pointer,
            CursorKind::Grab => CursorIcon::Grab,
            CursorKind::Grabbing => CursorIcon::Grabbing,
        };
        for w in self.wins.iter().flatten() {
            w.window.set_cursor(icon);
        }
    }

    fn set_tooltip(&mut self, k: WinKind, text: Option<&str>) {
        if let Some(w) = self.wins[idx(k)].as_ref() {
            os::tooltip(&w.window, text);
        }
    }

    fn app_open(&mut self, r: RectD, title: &str) {
        self.title = title.to_string();
        if let Some(w) = self.wins[idx(WinKind::App)].as_ref() {
            w.window.set_minimized(false);
            w.window.set_visible(true);
            w.window.focus_window();
            return;
        }
        if let Some(w) = self.create(WinKind::App, r, title) {
            w.visible = true;
            w.window.set_visible(true);
            w.window.focus_window();
            w.scale = w.window.scale_factor();
        }
    }

    fn app_close(&mut self) {
        self.wins[idx(WinKind::App)] = None;
        self.app_size = None;
    }

    fn app_is_open(&self) -> bool {
        self.wins[idx(WinKind::App)].is_some()
    }

    fn app_size(&self) -> Option<(f64, f64, f64)> {
        let w = self.wins[idx(WinKind::App)].as_ref()?;
        let s = w.window.scale_factor();
        let size = w.window.inner_size();
        Some((size.width as f64 / s, size.height as f64 / s, s))
    }

    fn app_present(&mut self, pm: &Pixmap) {
        if let Some(w) = self.wins[idx(WinKind::App)].as_mut() {
            w.last = Some(pm.clone());
            w.scale = w.window.scale_factor();
            os::present(w, pm, 1.0);
        }
    }

    fn app_title(&mut self, title: &str) {
        self.title = title.to_string();
        if let Some(w) = self.wins[idx(WinKind::App)].as_ref() {
            w.window.set_title(title);
        }
    }

    fn app_drag(&mut self) {
        if let Some(w) = self.wins[idx(WinKind::App)].as_ref() {
            let _ = w.window.drag_window();
        }
    }

    fn tray_tooltip(&mut self, text: &str) {
        self.tray.tooltip(text);
    }

    fn tray_menu(&mut self, open: &str, quit: &str) {
        self.tray.menu(open, quit);
    }

    fn login_item(&self) -> Option<bool> {
        os::login_item()
    }

    fn set_login_item(&mut self, on: bool) {
        os::set_login_item(on);
    }

    fn open_url(&mut self, url: &str) {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(url).spawn();
    }

    fn copy_text(&mut self, text: &str) {
        use std::io::Write;
        use std::process::{Command, Stdio};
        // pbcopy on macOS; on Linux whichever of Wayland's and X11's tools is there.
        let tools: &[(&str, &[&str])] =
            if cfg!(target_os = "macos") { &[("pbcopy", &[])] } else { &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["-ib"])] };
        for (cmd, args) in tools {
            if let Ok(mut child) = Command::new(cmd).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                // Reaped on another thread: xclip keeps running while it owns the selection.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return;
            }
        }
    }

    fn quit(&mut self) {
        self.quit = true;
    }

    fn relaunch(&mut self) {
        self.relaunch = true;
        self.quit = true;
    }

    fn set_timer(&mut self, after: Duration) {
        self.timer_at = Some(Instant::now() + after);
    }
}

struct Runner {
    app: Option<App>,
    host: Option<UnixHost>,
    fetcher: Fetcher,
    proxy: EventLoopProxy<UserEv>,
    _single: Option<SingleInstance>,
    wall: (Instant, SystemTime),
    _hotkey: Option<global_hotkey::GlobalHotKeyManager>,
}

impl Runner {
    /// Runs `f` with the app and the host pointing at the live event loop.
    fn with_app(&mut self, el: &ActiveEventLoop, f: impl FnOnce(&mut App, &mut UnixHost)) {
        let (Some(app), Some(host)) = (self.app.as_mut(), self.host.as_mut()) else { return };
        host.el = el;
        f(app, host);
        host.el = std::ptr::null();
        if host.quit {
            if host.relaunch {
                if let Ok(exe) = std::env::current_exe() {
                    drop(self._single.take());
                    let _ = std::process::Command::new(exe).arg("--relaunch").spawn();
                }
            }
            el.exit();
            return;
        }
        el.set_control_flow(match host.timer_at {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    fn ev(&mut self, el: &ActiveEventLoop, ev: Ev) {
        self.with_app(el, |app, host| app.event(host, ev));
    }
}

impl ApplicationHandler<UserEv> for Runner {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        let waker: crate::app::Waker = Arc::new(move || {
            let _ = proxy.send_event(UserEv::Wake);
        });
        let host = UnixHost {
            el: std::ptr::null(),
            wins: [None, None, None, None],
            timer_at: None,
            cursor: CursorKind::Default,
            tray: os::Tray::new(self.proxy.clone()),
            title: String::new(),
            quit: false,
            relaunch: false,
            proxy: self.proxy.clone(),
            app_size: None,
        };
        let _ = &host.proxy;
        let _ = host.app_size;
        self.host = Some(host);
        self.app = Some(App::new(self.fetcher, waker));
        // Ctrl+Alt+Q shows / hides the overlay.
        if let Ok(m) = global_hotkey::GlobalHotKeyManager::new() {
            use global_hotkey::hotkey::{Code, HotKey, Modifiers};
            if m.register(HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyQ)).is_ok() {
                let p = self.proxy.clone();
                global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(move |e: global_hotkey::GlobalHotKeyEvent| {
                    if e.state == global_hotkey::HotKeyState::Pressed {
                        let _ = p.send_event(UserEv::Hotkey);
                    }
                }));
            }
            self._hotkey = Some(m);
        }
        self.with_app(el, |app, host| app.start(host));
    }

    fn user_event(&mut self, el: &ActiveEventLoop, e: UserEv) {
        let ev = match e {
            UserEv::Wake => Ev::Wake,
            UserEv::Tray => Ev::TrayClick,
            UserEv::TrayOpen => Ev::TrayOpen,
            UserEv::TrayQuit => Ev::TrayQuit,
            UserEv::Hotkey => Ev::Hotkey,
            UserEv::Second => Ev::SecondInstance,
        };
        self.ev(el, ev);
    }

    fn new_events(&mut self, el: &ActiveEventLoop, cause: winit::event::StartCause) {
        // A jump of the wall clock past the monotonic one means the machine slept.
        let (i0, s0) = self.wall;
        let mono = i0.elapsed();
        let wall = s0.elapsed().unwrap_or(mono);
        self.wall = (Instant::now(), SystemTime::now());
        if wall > mono + Duration::from_secs(30) {
            self.ev(el, Ev::Resume);
        }
        if let winit::event::StartCause::ResumeTimeReached { .. } = cause {
            if let Some(h) = self.host.as_mut() {
                h.timer_at = None;
            }
            self.ev(el, Ev::Timer);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(host) = self.host.as_mut() else { return };
        let Some(kind) = host.kind_of(id) else { return };
        let mut out: Vec<Ev> = Vec::new();
        {
            let w = host.wins[idx(kind)].as_mut().unwrap();
            let s = w.window.scale_factor();
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    w.cursor = (position.x / s, position.y / s);
                    out.push(Ev::Mouse { w: kind, m: Mouse::Move, x: w.cursor.0, y: w.cursor.1 });
                }
                WindowEvent::CursorLeft { .. } => out.push(Ev::Mouse { w: kind, m: Mouse::Leave, x: -1.0, y: -1.0 }),
                WindowEvent::MouseInput { state, button, .. } => {
                    let (x, y) = w.cursor;
                    let b = match button {
                        MouseButton::Left => Btn3::Left,
                        MouseButton::Right => Btn3::Right,
                        _ => Btn3::Middle,
                    };
                    match state {
                        ElementState::Pressed => {
                            // The app window resizes from its edges (it has no system frame).
                            if kind == WinKind::App && b == Btn3::Left {
                                if let Some(dir) = edge(&w.window, x, y) {
                                    let _ = w.window.drag_resize_window(dir);
                                    return;
                                }
                            }
                            out.push(Ev::Mouse { w: kind, m: Mouse::Down(b), x, y });
                            if b == Btn3::Left {
                                let now = Instant::now();
                                let dbl = w.last_click.is_some_and(|(t, px, py)| {
                                    now.duration_since(t) < Duration::from_millis(500) && (px - x).abs() < 4.0 && (py - y).abs() < 4.0
                                });
                                if dbl {
                                    w.last_click = None;
                                    out.push(Ev::Mouse { w: kind, m: Mouse::Dbl(b), x, y });
                                } else {
                                    w.last_click = Some((now, x, y));
                                }
                            }
                        }
                        ElementState::Released => out.push(Ev::Mouse { w: kind, m: Mouse::Up(b), x, y }),
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let n = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => (p.y / 100.0) as f32,
                    };
                    out.push(Ev::Mouse { w: kind, m: Mouse::Wheel(n), x: w.cursor.0, y: w.cursor.1 });
                }
                WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                    let key = if event.logical_key == WKey::Named(NamedKey::Escape) { Key::Escape } else { Key::Other };
                    out.push(Ev::Key { w: kind, key });
                }
                WindowEvent::Focused(false) => out.push(Ev::Blur(kind)),
                WindowEvent::CloseRequested => out.push(Ev::Close(kind)),
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    if kind == WinKind::App {
                        out.push(Ev::Resized(kind));
                    }
                }
                WindowEvent::RedrawRequested => {
                    if let Some(pm) = w.last.clone() {
                        let a = w.alpha;
                        os::present(w, &pm, a);
                    }
                }
                _ => {}
            }
        }
        for e in out {
            self.ev(el, e);
        }
    }
}

/// Which edge of the frameless app window (6 px) is under (x, y), for resizing.
fn edge(w: &Window, x: f64, y: f64) -> Option<ResizeDirection> {
    let s = w.scale_factor();
    let size: PhysicalSize<u32> = w.inner_size();
    let (wd, hd) = (size.width as f64 / s, size.height as f64 / s);
    let b = 6.0;
    let (l, t, r, bt) = (x < b, y < b, x >= wd - b, y >= hd - b);
    Some(match (l, t, r, bt) {
        (true, true, _, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, _, true, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::West,
        (_, _, true, _) => ResizeDirection::East,
        (_, true, _, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

#[allow(dead_code)]
fn phys(x: f64, y: f64) -> PhysicalPosition<i32> {
    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

pub fn run(fetcher: Fetcher) {
    let relaunch = std::env::args().any(|a| a == "--relaunch");
    let mut builder = EventLoop::<UserEv>::with_user_event();
    os::configure(&mut builder);
    let Ok(el) = builder.build() else {
        eprintln!("Ai Quota Overlay: no display to open windows on");
        return;
    };
    let proxy = el.create_proxy();
    let single = match SingleInstance::acquire(relaunch, proxy.clone()) {
        Some(s) => s,
        None => return, // another copy runs and was told to show itself
    };
    let mut runner = Runner {
        app: None,
        host: None,
        fetcher,
        proxy,
        _single: Some(single),
        wall: (Instant::now(), SystemTime::now()),
        _hotkey: None,
    };
    let _ = el.run_app(&mut runner);
}

/// Single instance through a Unix socket: the first copy listens; a later start tells it to show itself and
/// exits, unless it was started by Restart (`--relaunch`), which waits for the old copy to let go.
pub struct SingleInstance {
    path: std::path::PathBuf,
}

impl SingleInstance {
    fn path() -> std::path::PathBuf {
        let user = std::env::var("USER").unwrap_or_default();
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(d) if !d.is_empty() => std::path::PathBuf::from(d).join("ai-quota-overlay.sock"),
            _ => std::env::temp_dir().join(format!("ai-quota-overlay-{user}.sock")),
        }
    }

    fn acquire(relaunch: bool, proxy: EventLoopProxy<UserEv>) -> Option<Self> {
        use std::io::Write;
        use std::os::unix::net::{UnixListener, UnixStream};
        let path = Self::path();
        let deadline = Instant::now() + Duration::from_secs(if relaunch { 15 } else { 0 });
        loop {
            match UnixListener::bind(&path) {
                Ok(listener) => {
                    std::thread::spawn(move || {
                        for s in listener.incoming().flatten() {
                            drop(s);
                            let _ = proxy.send_event(UserEv::Second);
                        }
                    });
                    return Some(Self { path });
                }
                Err(_) => match UnixStream::connect(&path) {
                    Ok(mut s) => {
                        if !relaunch || Instant::now() >= deadline {
                            let _ = s.write_all(b"show");
                            return None;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    // Nobody listens: a leftover from a crash.
                    Err(_) => {
                        let _ = std::fs::remove_file(&path);
                    }
                },
            }
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}