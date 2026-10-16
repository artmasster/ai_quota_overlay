#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod fmt;
mod http;
mod i18n;
mod model;
mod quota;
mod shots;
mod sqlite;
mod ui;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--screenshots") {
        let dir = args.get(i + 1).map(String::as_str).unwrap_or("docs");
        if let Err(e) = shots::run(std::path::Path::new(dir)) {
            eprintln!("screenshots: {e}");
            std::process::exit(1);
        }
    }
}
