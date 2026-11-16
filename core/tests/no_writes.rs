//! Static guard: only the allowed writers may use filesystem-mutating APIs:
//! `src-tauri/src/config.rs` (the app's own ~/.claudegate-desktop) and
//! `core/src/store.rs` (session file, lock, temp, backups). A trailing `#[cfg(test)] mod ...` block is
//! ignored, since unit tests write to temp dirs. A `#[cfg(test)]` on anything
//! else (a helper, a `use`) does not cut: the rest of the file is still scanned.

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
    "File::options",
    ".create(true)",
    ".write(true)",
    ".append(true)",
    "hard_link",
    "symlink(",
    "set_len",
    "write_all",
];

const ALLOWED: &[&str] = &["src-tauri/src/config.rs", "core/src/store.rs"];

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

/// Index of the first `#[cfg(test)]` line whose next non-empty line starts a
/// `mod`; everything from there on is test code. `lines.len()` if none.
fn cut_at(lines: &[&str]) -> usize {
    for (i, l) in lines.iter().enumerate() {
        if l.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[i + 1..].iter().map(|x| x.trim()).find(|x| !x.is_empty());
        if next.is_some_and(|n| n.starts_with("mod ") || n.starts_with("pub mod ")) {
            return i;
        }
    }
    lines.len()
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
        let lines: Vec<&str> = text.lines().collect();
        let end = cut_at(&lines);
        for (i, line) in lines[..end].iter().enumerate() {
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
        "write APIs outside the allowed writers:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn cut_only_at_cfg_test_mod() {
    // Shape 1: trailing test module cuts, code after it is test code.
    let a = ["fn a() {}", "", "#[cfg(test)]", "", "mod tests {", "  fs::write(x);", "}"];
    assert_eq!(cut_at(&a), 2);
    // Shape 2: cfg(test) on a helper does not cut; later code is still scanned.
    let b = ["#[cfg(test)]", "use std::fs;", "fn w() { fs::write(x, y); }"];
    assert_eq!(cut_at(&b), b.len());
    // Helper first, module later: cut at the module.
    let c = ["#[cfg(test)]", "fn h() {}", "fn real() {}", "#[cfg(test)]", "mod t {}"];
    assert_eq!(cut_at(&c), 3);
}
