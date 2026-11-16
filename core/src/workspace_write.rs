//! The only module that writes the user's workspace files: reject restores or deletes a file,
//! reapply writes Claude's version back. Each call names the session being decided and refuses
//! (`PermissionDenied`) any path that is not a key of that session's `files` or `rejected`, so a
//! bug elsewhere cannot point it at an arbitrary file. Keys are absolute (the hook resolves them);
//! a relative path is refused too, since it would resolve against this process's cwd.

use crate::model::Session;
use crate::store::atomic_write;
use std::fs;
use std::io;
use std::path::Path;

fn ensure_key(session: &Session, path: &Path) -> io::Result<()> {
    let held = path.is_absolute()
        && path.to_str().is_some_and(|k| session.files.contains_key(k) || session.rejected.contains_key(k));
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
pub fn restore(session: &Session, path: &Path, content: &str) -> io::Result<()> {
    ensure_key(session, path)?;
    atomic_write(path, content, true)
}

/// Delete `path` (the extension's `fs.unlinkSync`): a symlink is removed, not its target.
pub fn delete(session: &Session, path: &Path) -> io::Result<()> {
    ensure_key(session, path)?;
    fs::remove_file(path)
}
