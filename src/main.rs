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
mod sqlite;
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
    // Diagnostics: which ItemTable keys a SQLite file has (lengths only, never values).
    if let Some(i) = args.iter().position(|a| a == "--sqlite-keys") {
        let path = std::path::Path::new(args.get(i + 1).map(String::as_str).unwrap_or(""));
        let keys: Vec<&str> = args[i + 2..].iter().map(String::as_str).collect();
        let t = std::time::Instant::now();
        match sqlite::read_item_table(path, &keys) {
            Ok(m) => {
                for k in &keys {
                    println!("{k}: {:?}", m.get(*k).map(|v| v.len()));
                }
            }
            Err(e) => println!("error: {e}"),
        }
        println!("{:?}", t.elapsed());
        return;
    }
    platform::run(quota::fetch_all);
}