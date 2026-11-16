//! Sandbox-only driver for scripts/decide-e2e.sh: one `decide` against $CLAUDEGATE_HOME, with an
//! optional shell command run between the decision's load and its persist, the moment the
//! extension's hook can land a capture. Never run this against a real ClaudeGate home.
//!
//! Usage: decide_e2e <root> <desktop-home> <accept|reject> <file> [<shell command to run mid-decision>]
use claudegate_core::decide::{decide_with, Action, DecidePaths, Target};
use claudegate_core::paths::claudegate_home;
use std::path::Path;
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: decide_e2e <root> <desktop-home> <accept|reject> <file> [during-cmd]");
        std::process::exit(2);
    }
    let root = Path::new(&args[1]);
    let paths = DecidePaths { home: claudegate_home(), desktop_home: args[2].clone().into() };
    let action = match args[3].as_str() {
        "accept" => Action::Accept,
        "reject" => Action::Reject,
        other => {
            eprintln!("unknown action {other}");
            std::process::exit(2);
        }
    };
    let target = Target::File { scope: root.to_string_lossy().into_owned(), key: args[4].clone() };
    let during = args.get(5).cloned();
    let mut hook = |_: &Path| {
        if let Some(cmd) = &during {
            let ok = Command::new("sh").arg("-c").arg(cmd).status().map(|s| s.success()).unwrap_or(false);
            if !ok {
                eprintln!("the mid-decision command failed");
                std::process::exit(3);
            }
        }
    };
    // Real time: a decision stamped before the hook's capture would not supersede the stale disk copy.
    let now = claudegate_desktop_lib::commands::iso_from_ms(claudegate_core::time::now_ms());
    match decide_with(&paths, root, action, target, false, &now, &mut hook) {
        Ok(r) => println!("decided: done={} left={} errors={}", r.done, r.left.len(), r.errors.len()),
        Err(e) => {
            eprintln!("decide failed: {e}");
            std::process::exit(1);
        }
    }
}
