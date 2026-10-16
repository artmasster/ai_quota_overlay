//! macOS pieces of the winit host: screens in points (the DIP space Electron used), CALayer presenting,
//! window level and Spaces behaviour, tray (NSStatusItem through tray-icon) and a LaunchAgent for
//! "Launch at startup".
use super::{UserEv, Win};
use crate::app::{Display, RectD, WinKind};
use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaPremultipliedFirst, kCGRenderingIntentDefault};
use core_graphics::color_space::{kCGColorSpaceSRGB, CGColorSpace};
use core_graphics::data_provider::CGDataProvider;
use core_graphics::image::CGImage;
use foreign_types::ForeignType;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::path::PathBuf;
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::dpi::{LogicalPosition, LogicalSize, Position, Size};
use winit::event_loop::{ActiveEventLoop, EventLoopBuilder, EventLoopProxy};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS};
use winit::window::{Window, WindowAttributes};

pub struct WinData;

impl WinData {
    pub fn new(_w: &Window, _k: WinKind) -> Self {
        WinData
    }
}

pub fn configure(b: &mut EventLoopBuilder<UserEv>) {
    // No Dock icon (LSUIElement in Info.plist does the same for the bundle).
    b.with_activation_policy(ActivationPolicy::Accessory);
}

pub fn window_attrs(a: WindowAttributes, k: WinKind) -> WindowAttributes {
    if k == WinKind::App {
        a.with_titlebar_transparent(true).with_title_hidden(true).with_fullsize_content_view(true)
    } else {
        a.with_has_shadow(false)
    }
}

pub fn size_for(_el: &ActiveEventLoop, r: RectD) -> Size {
    LogicalSize::new(r.w, r.h).into()
}

pub fn pos_for(_el: &ActiveEventLoop, r: RectD) -> Position {
    LogicalPosition::new(r.x, r.y).into()
}

fn ns_view(w: &Window) -> Option<*mut AnyObject> {
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr() as *mut AnyObject),
        _ => None,
    }
}

struct Screen {
    frame: NSRect,
    visible: NSRect,
    scale: f64,
}

fn screens() -> Vec<Screen> {
    unsafe {
        let arr: *mut AnyObject = msg_send![class!(NSScreen), screens];
        if arr.is_null() {
            return Vec::new();
        }
        let n: usize = msg_send![arr, count];
        (0..n)
            .map(|i| {
                let s: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                let frame: NSRect = msg_send![s, frame];
                let visible: NSRect = msg_send![s, visibleFrame];
                let scale: f64 = msg_send![s, backingScaleFactor];
                Screen { frame, visible, scale }
            })
            .collect()
    }
}

/// Cocoa's bottom-left origin → top-left, relative to the primary (first) screen.
fn flip(r: NSRect, primary_h: f64) -> RectD {
    RectD::new(r.origin.x, primary_h - (r.origin.y + r.size.height), r.size.width, r.size.height)
}

pub fn displays(_el: &ActiveEventLoop) -> Vec<Display> {
    let ss = screens();
    let ph = ss.first().map(|s| s.frame.size.height).unwrap_or(0.0);
    ss.iter().map(|s| Display { bounds: flip(s.frame, ph), work: flip(s.visible, ph), scale: s.scale }).collect()
}

pub fn cursor(_el: &ActiveEventLoop) -> (f64, f64) {
    let ph = screens().first().map(|s| s.frame.size.height).unwrap_or(0.0);
    unsafe {
        let p: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        (p.x, ph - p.y)
    }
}

pub fn scale_for(el: &ActiveEventLoop, r: RectD) -> f64 {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    displays(el)
        .into_iter()
        .find(|d| d.bounds.contains(cx, cy))
        .or_else(|| displays(el).into_iter().next())
        .map(|d| d.scale)
        .unwrap_or(2.0)
}

pub fn place(w: &Window, r: RectD, pw: u32, ph: u32) {
    let s = (pw as f64 / r.w.max(1.0)).max(1.0);
    w.set_outer_position(LogicalPosition::new(r.x, r.y));
    let _ = w.request_inner_size(LogicalSize::new(pw as f64 / s, ph as f64 / s));
}

/// Above everything, on every Space and over full-screen apps (Electron: 'screen-saver' level, all workspaces).
pub fn after_show(w: &Window, k: WinKind) {
    if k == WinKind::App {
        return;
    }
    let Some(view) = ns_view(w) else { return };
    unsafe {
        let win: *mut AnyObject = msg_send![view, window];
        if win.is_null() {
            return;
        }
        let _: () = msg_send![win, setLevel: 1000isize];
        // canJoinAllSpaces (1) | stationary (16) | fullScreenAuxiliary (256)
        let _: () = msg_send![win, setCollectionBehavior: 1usize | 16 | 256];
        let _: () = msg_send![win, setHasShadow: false];
    }
}

pub fn present(w: &mut Win, pm: &Pixmap, alpha: f64) {
    let Some(view) = ns_view(&w.window) else { return };
    let (width, height) = (pm.width() as usize, pm.height() as usize);
    let a = alpha.clamp(0.0, 1.0);
    // Premultiplied RGBA → premultiplied BGRA (32-bit little endian, alpha first), opacity folded in.
    let mut data = Vec::with_capacity(width * height * 4);
    for p in pm.data().chunks_exact(4) {
        let k = |v: u8| ((v as f64) * a).round() as u8;
        data.extend_from_slice(&[k(p[2]), k(p[1]), k(p[0]), k(p[3])]);
    }
    let provider = CGDataProvider::from_buffer(Arc::new(data));
    let cs = CGColorSpace::create_with_name(unsafe { kCGColorSpaceSRGB }).unwrap_or_else(CGColorSpace::create_device_rgb);
    let image = CGImage::new(
        width,
        height,
        8,
        32,
        width * 4,
        &cs,
        kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little,
        &provider,
        false,
        kCGRenderingIntentDefault,
    );
    unsafe {
        let _: () = msg_send![view, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![view, layer];
        if layer.is_null() {
            return;
        }
        let img = image.as_ptr() as *mut AnyObject;
        let _: () = msg_send![layer, setContents: img];
        let _: () = msg_send![layer, setContentsScale: w.scale];
        // contentsGravity = resize (the image fills the view)
        let gravity = NSString::from_str("resize");
        let _: () = msg_send![layer, setContentsGravity: &*gravity];
    }
}

pub fn tooltip(w: &Window, text: Option<&str>) {
    let Some(view) = ns_view(w) else { return };
    unsafe {
        match text {
            Some(t) => {
                let s = NSString::from_str(t);
                let _: () = msg_send![view, setToolTip: &*s];
            }
            None => {
                let nil: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![view, setToolTip: nil];
            }
        }
    }
}

pub struct Tray {
    icon: Option<tray_icon::TrayIcon>,
    tip: String,
}

impl Tray {
    pub fn new(proxy: EventLoopProxy<UserEv>) -> Self {
        tray_icon::TrayIconEvent::set_event_handler(Some(move |e: tray_icon::TrayIconEvent| {
            if let tray_icon::TrayIconEvent::Click { button_state: tray_icon::MouseButtonState::Up, .. } = e {
                let _ = proxy.send_event(UserEv::Tray);
            }
        }));
        let icon = crate::ui::app_window::icon_pixmap().as_ref().and_then(|src| {
            let size = 36u32;
            let mut pm = Pixmap::new(size, size)?;
            let k = size as f32 / src.width() as f32;
            let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
            pm.draw_pixmap(0, 0, src.as_ref(), &paint, tiny_skia::Transform::from_scale(k, k), None);
            let rgba: Vec<u8> = pm.pixels().iter().flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            }).collect();
            tray_icon::Icon::from_rgba(rgba, size, size).ok()
        });
        let mut b = tray_icon::TrayIconBuilder::new().with_tooltip(crate::settings::APP_NAME).with_menu_on_left_click(false);
        if let Some(i) = icon {
            b = b.with_icon(i);
        }
        Self { icon: b.build().ok(), tip: crate::settings::APP_NAME.into() }
    }

    pub fn tooltip(&mut self, text: &str) {
        if self.tip != text {
            self.tip = text.to_string();
            if let Some(i) = &self.icon {
                let _ = i.set_tooltip(Some(text));
            }
        }
    }

    pub fn menu(&mut self, _open: &str, _quit: &str) {
        // macOS: a click on the status item opens the app window, as on Windows.
    }
}

const AGENT_ID: &str = "io.github.artmasster.ai-quota-overlay";

fn agent_path() -> PathBuf {
    crate::settings::home().join("Library/LaunchAgents").join(format!("{AGENT_ID}.plist"))
}

pub fn login_item() -> Option<bool> {
    Some(agent_path().exists())
}

pub fn set_login_item(on: bool) {
    let path = agent_path();
    if !on {
        let _ = std::fs::remove_file(path);
        return;
    }
    // The executable inside the .app bundle.
    let Ok(exe) = std::env::current_exe() else { return };
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{AGENT_ID}</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        esc(&exe.display().to_string())
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, plist);
}
