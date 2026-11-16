//! The only module that writes the user's workspace files: reject restores or deletes a file,
//! reapply writes Claude's version back. Each call names the session being decided and refuses
//! (`PermissionDenied`) any path that is not a key of that session's `files` (or, for `restore`,
//! `rejected`), so a bug elsewhere cannot point it at an arbitrary file. Keys are absolute (the hook resolves them);
//! a relative path is refused too, since it would resolve against this process's cwd.

use crate::model::Session;
use crate::store::atomic_write;
use std::fs;
use std::io;
use std::path::Path;

/// `rejected_too`: whether a key of `rejected` also qualifies (reapply writes a rejected path;
/// delete only ever serves a pending reject, so it is limited to `files`).
fn ensure_key(session: &Session, path: &Path, rejected_too: bool) -> io::Result<()> {
    let held = path.is_absolute()
        && path.to_str().is_some_and(|k| {
            session.files.contains_key(k) || (rejected_too && session.rejected.contains_key(k))
        });
    if held {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to write {}: it is not a file under review in this session", path.display()),
        ))
    }
}

/// Write `content` to `path` atomically, following a symlink and keeping the file's mode
/// (the extension's `atomicWrite(path, content, true)`).
pub(crate) fn restore(session: &Session, path: &Path, content: &str) -> io::Result<()> {
    ensure_key(session, path, true)?;
    atomic_write(path, content, true)
}

/// Delete `path` (the extension's `fs.unlinkSync`): a symlink is removed, not its target.
/// Only a pending key (`files`) may be deleted.
pub(crate) fn delete(session: &Session, path: &Path) -> io::Result<()> {
    ensure_key(session, path, false)?;
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    // Moved from tests/decide.rs when restore and delete became crate-private.
    use super::*;
    use crate::model::parse_session;
    use serde_json::json;

    fn session(files: serde_json::Value, rejected: serde_json::Value) -> Session {
        let v = json!({ "sessionId": "s", "status": "active", "files": files, "accepted": [], "rejected": rejected });
        parse_session(&v.to_string()).unwrap().0
    }

    fn pending() -> serde_json::Value {
        json!({ "originalContent": "old", "reviewStatus": "pending", "sessionId": "s1", "capturedAt": "2026-11-16T10:00:00.000Z" })
    }

    #[test]
    fn refuses_a_path_the_session_does_not_hold() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let other = tmp.path().join("other.txt");
        fs::write(&a, "keep").unwrap();
        fs::write(&other, "keep").unwrap();
        let s = session(json!({ a.to_str().unwrap(): pending() }), json!({}));
        assert_eq!(restore(&s, &other, "x").unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(delete(&s, &other).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&other).unwrap(), "keep");
        // A relative spelling of a key is refused too: it would resolve against our cwd.
        assert!(restore(&s, Path::new("a.txt"), "x").is_err());
        restore(&s, &a, "restored").unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), "restored");
    }

    #[test]
    fn delete_is_limited_to_pending_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        fs::write(&a, "keep").unwrap();
        let k = a.to_str().unwrap();
        let rec = json!({ "id": format!("t::{k}"), "path": k, "before": "o", "after": "keep", "decidedAt": "t", "sessionId": "s1" });
        let s = session(json!({}), json!({ k: rec }));
        assert_eq!(delete(&s, &a).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(a.exists());
        // restore may still write a rejected key (reapply).
        restore(&s, &a, "claude").unwrap();
    }
}
