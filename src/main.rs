#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod anim;
mod app;
mod cache;
mod fmt;
mod http;
mod i18n;
mod model;
mod platform;
mod quota;
mod settings;
mod shots;
mod ui;
mod updater;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--screenshots") {
        let dir = args.get(i + 1).map(String::as_str).unwrap_or("docs");
        if let Err(e) = shots::run(std::path::Path::new(dir)) {
            eprintln!("screenshots: {e}");
            std::process::exit(1);
        }
        return;
    }
    platform::run(quota::fetch_all);
}