use md5::{Digest, Md5};
use std::path::{Component, Path, PathBuf};

/// `$CLAUDEGATE_HOME` if set, else `~/.claudegate`.
pub fn claudegate_home() -> PathBuf {
    if let Some(h) = std::env::var_os("CLAUDEGATE_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    dirs::home_dir().unwrap_or_default().join(".claudegate")
}

fn root_hash(root: &Path) -> String {
    let resolved = resolve_root(root);
    #[allow(unused_mut)]
    let mut key = resolved.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        key = key.to_lowercase();
    }
    format!("{:x}", Md5::digest(key.as_bytes()))
}

/// `<home>/sessions/<md5(resolve_root(root))>.json`, the same name the extension and hook.py use.
pub fn session_file_for(home: &Path, root: &Path) -> PathBuf {
    home.join("sessions").join(format!("{}.json", root_hash(root)))
}

/// `<home>/audit/<md5(resolve_root(root))>.jsonl`, the capture audit's file for `root` (audit.py
/// hashes the root the same way as the session files).
pub fn audit_file_for(home: &Path, root: &Path) -> PathBuf {
    home.join("audit").join(format!("{}.jsonl", root_hash(root)))
}

/// Lexical absolute + normalised path, no symlink resolution (mirrors Node `path.resolve`).
pub fn resolve_root(root: &Path) -> PathBuf {
    let abs = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(root)
    };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never pop past the root or a prefix.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_extension() {
        let p = session_file_for(Path::new("/h"), Path::new("/ws/proj"));
        assert_eq!(p, Path::new("/h/sessions/3315a6271f371869636a76e1afc8237e.json"));
    }

    #[test]
    fn resolve_root_normalises() {
        assert_eq!(resolve_root(Path::new("/ws/./a/../proj/")), PathBuf::from("/ws/proj"));
    }

    #[test]
    fn home_honours_env() {
        std::env::set_var("CLAUDEGATE_HOME", "/tmp/x");
        assert_eq!(claudegate_home(), PathBuf::from("/tmp/x"));
        std::env::remove_var("CLAUDEGATE_HOME");
        let d = claudegate_home();
        assert!(d.ends_with(".claudegate"));
    }
}
