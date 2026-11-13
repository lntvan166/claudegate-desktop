//! Port of the extension's `src/excludeMatcher.ts` (v1.16.1). Read-only.
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// Translate a glob to an anchored regex. `**` -> `.*` (also eating a following `/`),
/// `*` -> `[^/]*`, `?` -> `[^/]`. `None` if the result does not compile (fail open).
pub fn glob_to_regex(glob: &str) -> Option<Regex> {
    let g: Vec<char> = glob.replace('\\', "/").chars().collect();
    let mut re = String::from("^");
    let mut i = 0;
    while i < g.len() {
        let c = g[i];
        if c == '*' {
            if g.get(i + 1) == Some(&'*') {
                re.push_str(".*");
                i += 1;
                if g.get(i + 1) == Some(&'/') {
                    i += 1;
                }
            } else {
                re.push_str("[^/]*");
            }
        } else if c == '?' {
            re.push_str("[^/]");
        } else if "\\^$.|+()[]{}".contains(c) {
            re.push('\\');
            re.push(c);
        } else {
            re.push(c);
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).ok()
}

pub struct ExcludeMatcher {
    patterns: Vec<Regex>,
    root: String,
}

impl ExcludeMatcher {
    pub fn new(root: &Path, globs: &[String]) -> Self {
        ExcludeMatcher {
            patterns: globs.iter().filter_map(|g| glob_to_regex(g)).collect(),
            root: root.to_string_lossy().replace('\\', "/"),
        }
    }

    /// Tests the absolute path, the root-relative path and every ancestor dir of the relative path.
    pub fn is_excluded(&self, path: &str) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        let abs = path.replace('\\', "/");
        let mut cands: Vec<String> = vec![abs.clone()];
        if !self.root.is_empty() {
            if let Some(rel) = abs.strip_prefix(&format!("{}/", self.root)) {
                cands.push(rel.to_string());
                let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
                for i in 1..parts.len() {
                    cands.push(parts[..i].join("/"));
                }
            }
        }
        self.patterns.iter().any(|re| cands.iter().any(|c| re.is_match(c)))
    }
}

/// `<home>/excludes.json`: `{ "<root>": ["glob", ...] }`. Missing / invalid -> empty.
pub fn read_excludes(home: &Path) -> HashMap<String, Vec<String>> {
    let Ok(raw) = std::fs::read_to_string(home.join("excludes.json")) else {
        return HashMap::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::Array(a) => Some((
                k,
                a.into_iter().filter_map(|g| g.as_str().map(String::from)).collect(),
            )),
            _ => None,
        })
        .collect()
}
