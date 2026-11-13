//! scripts/readonly-proof.sh must prove the binary that was just built, not a stale one.
//! Each case copies the script into a fake repo whose target/ binaries have chosen mtimes and
//! runs it with `--print-bin`, which reports the choice and exits before touching any home.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

struct Repo {
    dir: tempfile::TempDir,
}

fn ago(secs: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(secs)
}

fn put(path: &Path, mtime: SystemTime) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "#!/bin/sh\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    fs::File::options().write(true).open(path).unwrap().set_modified(mtime).unwrap();
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/readonly-proof.sh");
        let to = dir.path().join("scripts/readonly-proof.sh");
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(script, &to).unwrap();
        // sources older than any binary unless a test says otherwise
        for s in ["core/src/lib.rs", "src-tauri/src/lib.rs", "ui/src/App.svelte"] {
            put(&dir.path().join(s), ago(10_000));
        }
        Repo { dir }
    }
    fn bin(&self, profile: &str) -> PathBuf {
        self.dir.path().join("target").join(profile).join("claudegate-desktop")
    }
    fn run(&self, args: &[&str]) -> String {
        let out = Command::new("bash")
            .arg(self.dir.path().join("scripts/readonly-proof.sh"))
            .arg("--print-bin")
            .args(args)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "script failed: {text}");
        text
    }
}

fn chosen(out: &str) -> &str {
    out.lines()
        .find_map(|l| l.strip_prefix("binary: "))
        .and_then(|l| l.split(" (built ").next())
        .unwrap_or_else(|| panic!("no 'binary:' line in:\n{out}"))
}

#[test]
fn newer_debug_beats_older_release() {
    let r = Repo::new();
    put(&r.bin("release"), ago(3_000));
    put(&r.bin("debug"), ago(60));
    let out = r.run(&[]);
    assert_eq!(chosen(&out), r.bin("debug").to_string_lossy());
    assert!(!out.contains("WARNING"), "{out}");
}

#[test]
fn newer_release_beats_older_debug() {
    let r = Repo::new();
    put(&r.bin("release"), ago(60));
    put(&r.bin("debug"), ago(3_000));
    assert_eq!(chosen(&r.run(&[])), r.bin("release").to_string_lossy());
}

#[test]
fn explicit_bin_wins() {
    let r = Repo::new();
    put(&r.bin("release"), ago(3_000));
    put(&r.bin("debug"), ago(60));
    let mine = r.dir.path().join("elsewhere/claudegate-desktop");
    put(&mine, ago(30));
    let out = r.run(&["--bin", &mine.to_string_lossy()]);
    assert_eq!(chosen(&out), mine.to_string_lossy());
}

#[test]
fn prints_head_and_warns_when_sources_are_newer() {
    let r = Repo::new();
    put(&r.bin("debug"), ago(3_000));
    put(&r.dir.path().join("core/src/project.rs"), ago(5));
    let out = r.run(&[]);
    assert!(out.lines().any(|l| l.starts_with("HEAD: ")), "{out}");
    assert!(out.contains("WARNING") && out.contains("core/src/project.rs"), "{out}");
}
