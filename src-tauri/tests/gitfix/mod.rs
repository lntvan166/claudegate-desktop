//! Real git fixture repositories with fixed dates, for the gitlog tests.
#![allow(dead_code)]

use claudegate_core::gitlog::{Checkout, Repo};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 2026-01-01T00:00:00Z.
pub const BASE: i64 = 1_767_225_600;

fn cmd(dir: &Path, t: i64) -> Command {
    let mut c = Command::new("git");
    c.args(["-c", "user.name=Ann Dev", "-c", "user.email=ann@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .current_dir(dir)
        .env("GIT_COMMITTER_DATE", format!("@{t} +0000"))
        .env("GIT_AUTHOR_DATE", format!("@{t} +0000"))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    c
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = cmd(dir, BASE).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub fn init(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
}

/// Writes (`Some`) or deletes (`None`) each file, then commits everything at time `t`.
/// Returns the new commit's sha.
/// An empty commit by author `name` at time `t`.
pub fn commit_by(dir: &Path, name: &str, msg: &str, t: i64) -> String {
    let out = cmd(dir, t).env("GIT_AUTHOR_NAME", name).args(["commit", "-q", "--allow-empty", "-m", msg]).output().unwrap();
    assert!(out.status.success(), "commit: {}", String::from_utf8_lossy(&out.stderr));
    git(dir, &["rev-parse", "HEAD"])
}

pub fn commit(dir: &Path, files: &[(&str, Option<&[u8]>)], msg: &str, t: i64) -> String {
    commit_with(dir, files, msg, t, None)
}

/// [`commit`] by `name` (author and committer, email `{name}@example.com`).
pub fn commit_as(dir: &Path, name: &str, files: &[(&str, Option<&[u8]>)], msg: &str, t: i64) -> String {
    commit_with(dir, files, msg, t, Some(name))
}

fn commit_with(dir: &Path, files: &[(&str, Option<&[u8]>)], msg: &str, t: i64, name: Option<&str>) -> String {
    for (p, content) in files {
        let f = dir.join(p);
        match content {
            Some(c) => {
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(&f, c).unwrap();
            }
            None => {
                let _ = std::fs::remove_file(&f);
            }
        }
    }
    let out = cmd(dir, t).args(["add", "-A"]).output().unwrap();
    assert!(out.status.success());
    let mut c = cmd(dir, t);
    if let Some(n) = name {
        let email = format!("{n}@example.com");
        c.env("GIT_AUTHOR_NAME", n).env("GIT_COMMITTER_NAME", n).env("GIT_AUTHOR_EMAIL", &email).env("GIT_COMMITTER_EMAIL", &email);
    }
    let out = c.args(["commit", "-q", "--allow-empty", "-m", msg]).output().unwrap();
    assert!(out.status.success(), "commit: {}", String::from_utf8_lossy(&out.stderr));
    git(dir, &["rev-parse", "HEAD"])
}

/// `n` commits on `main` through `git fast-import` (fast enough for hundreds), each changing
/// one of ten files. `time(k)` is the time of the k-th newest commit (0 is the tip). HEAD is
/// left on `main`.
pub fn bulk(dir: &Path, n: usize, time: impl Fn(usize) -> i64) {
    init(dir);
    let mut s = String::new();
    for i in 0..n {
        let t = time(n - 1 - i);
        let msg = format!("change {i}\n");
        let body = format!("line {i}\n").repeat(20);
        s.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
        s.push_str(&format!("author Ann Dev <ann@example.com> {t} +0000\ncommitter Ann Dev <ann@example.com> {t} +0000\n"));
        s.push_str(&format!("data {}\n{msg}", msg.len()));
        if i > 0 {
            s.push_str(&format!("from :{i}\n"));
        }
        s.push_str(&format!("M 100644 inline src/f{}.txt\ndata {}\n{body}\n", i % 10, body.len()));
    }
    let mut child = cmd(dir, BASE).args(["fast-import", "--quiet"]).stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
}

pub fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// A one-checkout `Repo` for `dir`, without discovery (works for broken repositories too).
pub fn repo_at(dir: &Path) -> Repo {
    let path = dir.to_string_lossy().into_owned();
    Repo {
        id: format!("{path}/.git"),
        name: dir.file_name().unwrap().to_string_lossy().into_owned(),
        color: 0,
        checkouts: vec![Checkout { path, head: "0".repeat(40), branch: Some("main".into()), is_main: true }],
    }
}

pub fn tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = canon(t.path());
    (t, p)
}
