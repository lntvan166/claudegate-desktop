use claudegate_core::excludes::{glob_to_regex, ExcludeMatcher};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[test]
fn fixture_cases() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/exclude_cases.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let root = v["root"].as_str().unwrap();
    let cases = v["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let globs: Vec<String> =
            c["globs"].as_array().unwrap().iter().map(|g| g.as_str().unwrap().to_string()).collect();
        let m = ExcludeMatcher::new(Path::new(root), &globs);
        let path = c["path"].as_str().unwrap();
        assert_eq!(m.is_excluded(path), c["excluded"].as_bool().unwrap(), "{globs:?} {path}");
    }
}

#[test]
fn glob_basics() {
    let re = glob_to_regex("**/x").unwrap();
    assert!(re.is_match("x") && re.is_match("a/b/x")); // faithful to the TS: `**/` eats the slash, so "a/bx" also matches
    assert!(glob_to_regex("a?c").unwrap().is_match("abc"));
    assert!(!glob_to_regex("a?c").unwrap().is_match("a/c"));
    assert!(!glob_to_regex("*.js").unwrap().is_match("a/b.js"));
    assert!(glob_to_regex("a.b").unwrap().is_match("a.b") && !glob_to_regex("a.b").unwrap().is_match("axb"));
}

#[test]
fn backslashes_normalised() {
    let m = ExcludeMatcher::new(Path::new("/ws/proj"), &["dist".to_string()]);
    assert!(m.is_excluded("\\ws\\proj\\dist\\a.js"));
}
