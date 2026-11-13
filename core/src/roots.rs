//! Reader for the extension's shared `workspace-roots.json` (read-only).
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;

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
