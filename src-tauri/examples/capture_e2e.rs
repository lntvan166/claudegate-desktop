//! Sandbox-only driver for scripts/capture-e2e.sh: adds a project the way the app does, sets
//! capture up, runs the startup hook update and Update hook, all with the app's plain functions. It writes
//! `$HOME/.claude/settings.json` and `$HOME/.claudegate`, so it exits 2 unless CLAUDEGATE_HOME
//! and CLAUDEGATE_DESKTOP_CONFIG are both set and both inside $HOME (the script's temp HOME).
//!
//! Usage: capture_e2e add <dir> | setup | update | update-hook | status
use claudegate_desktop_lib::capture::{
    capture_view_for, python3_cached, set_up_capture_with, update_capture_hook_with, update_hook_with,
};
use claudegate_desktop_lib::commands::add_project_with;
use claudegate_desktop_lib::config::config_path;
use std::path::PathBuf;

fn fail(code: i32, msg: impl std::fmt::Display) -> ! {
    eprintln!("capture_e2e: {msg}");
    std::process::exit(code);
}

fn main() {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let Some(home) = home else { fail(2, "HOME is not set; refusing to run outside a sandbox") };
    for var in ["CLAUDEGATE_HOME", "CLAUDEGATE_DESKTOP_CONFIG"] {
        match std::env::var_os(var).filter(|v| !v.is_empty()) {
            None => fail(2, format!("{var} is not set; refusing to run outside a sandbox")),
            Some(v) if !PathBuf::from(&v).starts_with(&home) => {
                fail(2, format!("{var} is not inside HOME; refusing to run outside a sandbox"))
            }
            Some(_) => {}
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = config_path();
    match args.first().map(String::as_str) {
        Some("add") if args.len() == 2 => {
            let cg = claudegate_core::paths::claudegate_home();
            let added = add_project_with(&cg, &cfg, &args[1]).unwrap_or_else(|e| fail(1, e));
            println!("added {}", added.root);
        }
        Some("setup") => {
            let v = set_up_capture_with(&home, &cfg, python3_cached(true)).unwrap_or_else(|e| fail(1, e));
            println!("{}", serde_json::to_string(&v).expect("serializable"));
        }
        Some("update") => match update_hook_with(&home, &cfg, python3_cached(true)) {
            Ok(r) => println!("{r:?}"),
            Err(e) => fail(1, e),
        },
        Some("update-hook") => {
            let v = update_capture_hook_with(&home, &cfg, python3_cached(true)).unwrap_or_else(|e| fail(1, e));
            println!("{}", serde_json::to_string(&v).expect("serializable"));
        }
        Some("status") => {
            println!("{}", serde_json::to_string(&capture_view_for(&home, python3_cached(true))).expect("serializable"))
        }
        _ => fail(2, "usage: capture_e2e add <dir> | setup | update | update-hook | status"),
    }
}
