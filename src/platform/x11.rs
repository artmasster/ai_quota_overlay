//! Linux pieces of the winit host (X11, also under XWayland): monitors and the work area (_NET_WORKAREA),
//! the pointer, PutImage presenting into 32-bit ARGB windows, override-redirect overlays, and the
//! StatusNotifierItem tray (ksni) with a small menu, since Linux trays don't report clicks everywhere.
use super::{UserEv, Win};
use crate::app::{Display, RectD, WinKind};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::OnceLock;
use tiny_skia::Pixmap;
use winit::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use winit::event_loop::{ActiveEventLoop, EventLoopBuilder, EventLoopProxy};
use winit::platform::x11::{EventLoopBuilderExtX11, WindowAttributesExtX11, WindowType};
use winit::window::{Window, WindowAttributes};
use x11rb::connection::Connection;
use x11rb::image::{BitsPerPixel, Image, ImageOrder, ScanlinePad};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateGCAux, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

fn conn() -> Option<&'static (RustConnection, usize)> {
    static C: OnceLock<Option<(RustConnection, usize)>> = OnceLock::new();
    C.get_or_init(|| RustConnection::connect(None).ok()).as_ref()
}

pub struct WinData {
    xid: u32,
    gc: u32,
    depth: u8,
}

impl WinData {
    pub fn new(w: &Window, k: WinKind) -> Self {
        let xid = match w.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
            Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
            _ => 0,
        };
        let (mut gc, mut depth) = (0, 24);
        if let Some((c, _)) = conn() {
            if xid != 0 {
                if let Ok(g) = c.get_geometry(xid).and_then(|r| Ok(r.reply())) {
                    if let Ok(g) = g {
                        depth = g.depth;
                    }
                }
                if let Ok(id) = c.generate_id() {
                    if c.create_gc(id, xid, &CreateGCAux::new()).is_ok() {
                        gc = id;
                    }
                }
                if k == WinKind::Panel {
                    // Keep the detail panel out of taskbars and pagers.
                    let atom = |n: &str| c.intern_atom(false, n.as_bytes()).ok().and_then(|r| r.reply().ok()).map(|r| r.atom);
                    if let (Some(state), Some(skip), Some(pager)) =
                        (atom("_NET_WM_STATE"), atom("_NET_WM_STATE_SKIP_TASKBAR"), atom("_NET_WM_STATE_SKIP_PAGER"))
                    {
                        let _ = c.change_property32(PropMode::APPEND, xid, state, AtomEnum::ATOM, &[skip, pager]);
                    }
                }
                let _ = c.flush();
            }
        }
        WinData { xid, gc, depth }
    }
}

pub fn configure(b: &mut EventLoopBuilder<UserEv>) {
    // Overlays need absolute positions and stacking, which Wayland doesn't give clients: run on X11 (XWayland).
    b.with_x11();
}

pub fn window_attrs(a: WindowAttributes, k: WinKind) -> WindowAttributes {
    let a = a.with_name("ai-quota-overlay", "Ai Quota Overlay");
    match k {
        // Unmanaged: never focused, no taskbar entry, placed exactly.
        WinKind::Card | WinKind::Toast => a.with_override_redirect(true),
        WinKind::Panel => a.with_x11_window_type(vec![WindowType::Utility]),
        WinKind::App => a,
    }
}

struct Mon {
    phys: (i32, i32, u32, u32),
    scale: f64,
}

fn mons(el: &ActiveEventLoop) -> Vec<Mon> {
    let mut v: Vec<Mon> = el
        .available_monitors()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            Mon { phys: (p.x, p.y, s.width, s.height), scale: m.scale_factor() }
        })
        .collect();
    if let Some(p) = el.primary_monitor() {
        let pp = p.position();
        if let Some(i) = v.iter().position(|m| (m.phys.0, m.phys.1) == (pp.x, pp.y)) {
            let m = v.remove(i);
            v.insert(0, m);
        }
    }
    v
}

/// _NET_WORKAREA of the current desktop (physical px), if the window manager publishes it.
fn workarea() -> Option<(i32, i32, i32, i32)> {
    let (c, screen) = conn()?;
    let root = c.setup().roots.get(*screen)?.root;
    let atom = c.intern_atom(false, b"_NET_WORKAREA").ok()?.reply().ok()?.atom;
    let r = c.get_property(false, root, atom, AtomEnum::CARDINAL, 0, 4).ok()?.reply().ok()?;
    let v: Vec<u32> = r.value32()?.collect();
    (v.len() >= 4).then(|| (v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32))
}

pub fn displays(el: &ActiveEventLoop) -> Vec<Display> {
    let wa = workarea();
    mons(el)
        .iter()
        .map(|m| {
            let (x, y, w, h) = m.phys;
            let s = m.scale;
            let bounds = RectD::new(x as f64 / s, y as f64 / s, w as f64 / s, h as f64 / s);
            let work = match wa {
                Some((wx, wy, ww, wh)) => {
                    let l = x.max(wx);
                    let t = y.max(wy);
                    let r = (x + w as i32).min(wx + ww);
                    let b = (y + h as i32).min(wy + wh);
                    if r > l && b > t {
                        RectD::new(l as f64 / s, t as f64 / s, (r - l) as f64 / s, (b - t) as f64 / s)
                    } else {
                        bounds
                    }
                }
                None => bounds,
            };
            Display { bounds, work, scale: s }
        })
        .collect()
}

pub fn scale_for(el: &ActiveEventLoop, r: RectD) -> f64 {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let ds = displays(el);
    ds.iter().find(|d| d.bounds.contains(cx, cy)).or(ds.first()).map(|d| d.scale).unwrap_or(1.0)
}

pub fn size_for(el: &ActiveEventLoop, r: RectD) -> Size {
    let s = scale_for(el, r);
    PhysicalSize::new((r.w * s).round().max(1.0) as u32, (r.h * s).round().max(1.0) as u32).into()
}

pub fn pos_for(el: &ActiveEventLoop, r: RectD) -> Position {
    let s = scale_for(el, r);
    PhysicalPosition::new((r.x * s).round() as i32, (r.y * s).round() as i32).into()
}

pub fn cursor(el: &ActiveEventLoop) -> (f64, f64) {
    let Some((c, screen)) = conn() else { return (0.0, 0.0) };
    let Some(root) = c.setup().roots.get(*screen).map(|s| s.root) else { return (0.0, 0.0) };
    let Some(p) = c.query_pointer(root).ok().and_then(|r| r.reply().ok()) else { return (0.0, 0.0) };
    let (x, y) = (p.root_x as i32, p.root_y as i32);
    for m in mons(el) {
        let (mx, my, mw, mh) = m.phys;
        if x >= mx && x < mx + mw as i32 && y >= my && y < my + mh as i32 {
            return (x as f64 / m.scale, y as f64 / m.scale);
        }
    }
    (x as f64, y as f64)
}

pub fn place(w: &Window, r: RectD, pw: u32, ph: u32) {
    let s = (pw as f64 / r.w.max(1.0)).max(0.1);
    w.set_outer_position(PhysicalPosition::new((r.x * s).round() as i32, (r.y * s).round() as i32));
    let _ = w.request_inner_size(PhysicalSize::new(pw, ph));
}

pub fn after_show(w: &Window, k: WinKind) {
    // Unmanaged overlays stay above by restacking when shown.
    if matches!(k, WinKind::Card | WinKind::Toast) {
        if let (Some((c, _)), Ok(RawWindowHandle::Xlib(h))) = (conn(), w.window_handle().map(|h| h.as_raw())) {
            let aux = x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
            let _ = c.configure_window(h.window as u32, &aux);
            let _ = c.flush();
        }
    }
}

pub fn present(w: &mut Win, pm: &Pixmap, alpha: f64) {
    let Some((c, _)) = conn() else { return };
    let d = &w.os;
    if d.xid == 0 || d.gc == 0 {
        return;
    }
    let a = alpha.clamp(0.0, 1.0);
    let mut data = Vec::with_capacity(pm.data().len());
    for p in pm.data().chunks_exact(4) {
        let k = |v: u8| ((v as f64) * a).round() as u8;
        // ARGB visuals take premultiplied pixels, little-endian BGRA.
        let alpha = if d.depth == 32 { k(p[3]) } else { 255 };
        data.extend_from_slice(&[k(p[2]), k(p[1]), k(p[0]), alpha]);
    }
    let Ok(img) = Image::new(
        pm.width() as u16,
        pm.height() as u16,
        ScanlinePad::Pad32,
        d.depth,
        BitsPerPixel::B32,
        ImageOrder::LsbFirst,
        data.into(),
    ) else {
        return;
    };
    let _ = img.put(c, d.xid, d.gc, 0, 0);
    let _ = c.flush();
}

pub fn tooltip(_w: &Window, _text: Option<&str>) {}

struct SniTray {
    tip: String,
    open: String,
    quit: String,
    proxy: EventLoopProxy<UserEv>,
    icon: Vec<ksni::Icon>,
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "ai-quota-overlay".into()
    }
    fn title(&self) -> String {
        crate::settings::APP_NAME.into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: crate::settings::APP_NAME.into(), description: self.tip.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.proxy.send_event(UserEv::Tray);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.open.clone(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.proxy.send_event(UserEv::TrayOpen);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: self.quit.clone(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.proxy.send_event(UserEv::TrayQuit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct Tray {
    handle: Option<ksni::blocking::Handle<SniTray>>,
}

impl Tray {
    pub fn new(proxy: EventLoopProxy<UserEv>) -> Self {
        use ksni::blocking::TrayMethods;
        let icon = crate::ui::app_window::icon_pixmap()
            .as_ref()
            .map(|src| {
                let size = 32u32;
                let mut pm = Pixmap::new(size, size).unwrap();
                let k = size as f32 / src.width() as f32;
                let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
                pm.draw_pixmap(0, 0, src.as_ref(), &paint, tiny_skia::Transform::from_scale(k, k), None);
                // ARGB32, network byte order.
                let data = pm
                    .pixels()
                    .iter()
                    .flat_map(|p| {
                        let c = p.demultiply();
                        [c.alpha(), c.red(), c.green(), c.blue()]
                    })
                    .collect();
                vec![ksni::Icon { width: size as i32, height: size as i32, data }]
            })
            .unwrap_or_default();
        let m = &crate::i18n::EN.menu;
        let tray = SniTray { tip: String::new(), open: m.settings.into(), quit: m.quit.into(), proxy, icon };
        Self { handle: tray.spawn().ok() }
    }

    pub fn tooltip(&mut self, text: &str) {
        if let Some(h) = &self.handle {
            let t = text.to_string();
            h.update(move |s| s.tip = t);
        }
    }

    pub fn menu(&mut self, open: &str, quit: &str) {
        if let Some(h) = &self.handle {
            let (o, q) = (open.to_string(), quit.to_string());
            h.update(move |s| {
                s.open = o;
                s.quit = q;
            });
        }
    }
}

/// Launch at startup isn't offered on Linux (as before).
pub fn login_item() -> Option<bool> {
    None
}

pub fn set_login_item(_on: bool) {}
