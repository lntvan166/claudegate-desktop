//! Static guard: the viewer must never write under the ClaudeGate home.
//! Only `src-tauri/src/config.rs` (the app's own ~/.claudegate-desktop) may
//! use filesystem-mutating APIs. Code after the first `#[cfg(test)]` line in a
//! file is ignored, since unit tests write to temp dirs.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "fs::write",
    "File::create",
    "OpenOptions",
    "rename(",
    "remove_file",
    "remove_dir",
    "create_dir",
    "fs::copy",
    "set_permissions",
];

const ALLOWED: &[&str] = &["src-tauri/src/config.rs"];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn scan(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rs_files(&root.join("core/src"), &mut files);
    rs_files(&root.join("src-tauri/src"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "scanned no source files; wrong root?");
    let mut offenders = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        let text = fs::read_to_string(f).unwrap();
        for (i, line) in text.lines().enumerate() {
            if line.contains("#[cfg(test)]") {
                break;
            }
            for tok in FORBIDDEN {
                if line.contains(tok) {
                    offenders.push(format!("{}:{}: {}", rel, i + 1, tok));
                }
            }
        }
    }
    offenders
}

#[test]
fn only_config_rs_may_write() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let offenders = scan(root);
    assert!(
        offenders.is_empty(),
        "write APIs outside src-tauri/src/config.rs:\n{}",
        offenders.join("\n")
    );
}
