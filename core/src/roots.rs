//! Reader for the extension's shared `workspace-roots.json` (read-only), and the nesting guard
//! the app applies before it adds a root. The writers are `store::roots_add` / `roots_remove`.
use std::path::{Path, PathBuf};

/// Why a project could not be added or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootsError {
    /// The folder is inside `existing`. `worktree`: it is a git worktree, which needs no root.
    Nested { existing: String, worktree: bool },
    /// The folder contains `existing`.
    Contains { existing: String },
    NotADirectory,
    NotAbsolute,
    /// Only roots the app added can be removed; VS Code's roots are the extension's.
    NotAppAdded,
    Io(String),
}

impl std::fmt::Display for RootsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RootsError::Nested { existing, worktree: true } => write!(
                f,
                "This folder is a git worktree inside {existing}. It needs no project of its own: worktrees inside a project are found automatically."
            ),
            RootsError::Nested { existing, worktree: false } => write!(
                f,
                "This folder is inside {existing}, which is already a project. Its edits are captured there."
            ),
            RootsError::Contains { existing } => write!(
                f,
                "This folder contains {existing}, which is already a project. Add a folder that does not overlap it."
            ),
            RootsError::NotADirectory => write!(f, "That is not a folder that exists."),
            RootsError::NotAbsolute => write!(f, "Give the full path of the folder, starting from /."),
            RootsError::NotAppAdded => {
                write!(f, "This project comes from VS Code. Remove it there; the app only removes projects it added.")
            }
            RootsError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RootsError {}

/// The nesting guard, pure: `hook.py` routes a capture to the longest matching root, so a root
/// nested inside (or around) another would pull captures out of a session VS Code shows.
/// Compares whole path components of the resolved forms. Equal to an existing root is fine
/// (adding it again is a no-op).
pub fn check_new_root(existing: &[PathBuf], candidate: &Path) -> Result<(), RootsError> {
    let c = crate::paths::resolve_root(candidate);
    for e in existing {
        let e = crate::paths::resolve_root(e);
        if e == c {
            return Ok(());
        }
    }
    for e in existing {
        let e = crate::paths::resolve_root(e);
        let name = e.to_string_lossy().into_owned();
        if c.starts_with(&e) {
            return Err(RootsError::Nested { existing: name, worktree: false });
        }
        if e.starts_with(&c) {
            return Err(RootsError::Contains { existing: name });
        }
    }
    Ok(())
}

/// `<home>/workspace-roots.json`: a JSON array of strings. Missing / invalid / non-list -> empty;
/// non-string items are skipped.
pub fn read_roots(home: &Path) -> Vec<PathBuf> {
    let Ok(raw) = std::fs::read_to_string(home.join("workspace-roots.json")) else {
        return Vec::new();
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(serde_json::Value::Array(a)) => {
            a.into_iter().filter_map(|v| v.as_str().map(PathBuf::from)).collect()
        }
        _ => Vec::new(),
    }
}

/// `read_roots` for a caller that must not act on a bad read: a missing file is no roots, but a
/// file that cannot be read, is not JSON (the extension writes it in place, so it can be caught
/// half-written) or is not a list of strings is an error.
pub fn read_roots_strict(home: &Path) -> Result<Vec<PathBuf>, String> {
    let file = home.join("workspace-roots.json");
    let raw = match std::fs::read_to_string(&file) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Cannot read {}: {e}", file.display())),
    };
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("{} is not valid JSON ({e}).", file.display()))?;
    v.as_array()
        .and_then(|a| a.iter().map(|x| x.as_str().map(PathBuf::from)).collect::<Option<Vec<_>>>())
        .ok_or_else(|| format!("{} is not a list of folders.", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strict_read_tells_missing_from_broken() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_roots_strict(d.path()), Ok(vec![]));
        let f = d.path().join("workspace-roots.json");
        std::fs::write(&f, r#"["/a","/b"]"#).unwrap();
        assert_eq!(read_roots_strict(d.path()), Ok(vec![PathBuf::from("/a"), PathBuf::from("/b")]));
        for bad in [r#"["/a", "/b"#, r#"{"a":1}"#, r#"["/a", 2]"#, ""] {
            std::fs::write(&f, bad).unwrap();
            assert!(read_roots_strict(d.path()).is_err(), "{bad}");
        }
    }

    fn with(content: Option<&str>) -> Vec<PathBuf> {
        let d = tempfile::tempdir().unwrap();
        if let Some(c) = content {
            std::fs::write(d.path().join("workspace-roots.json"), c).unwrap();
        }
        read_roots(d.path())
    }

    #[test]
    fn skips_non_strings() {
        assert_eq!(with(Some(r#"["/a", 3, "/b"]"#)), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn non_list_missing_invalid_are_empty() {
        assert!(with(Some("{}")).is_empty());
        assert!(with(None).is_empty());
        assert!(with(Some("not json")).is_empty());
    }
}
