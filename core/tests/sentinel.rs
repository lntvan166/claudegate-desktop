//! The hook-log sentinel: the only file in the ClaudeGate home the settings screen may touch.
//! One test in its own binary, because it sets `CLAUDEGATE_HOME` for the process.
use claudegate_core::store::set_hook_log;
use std::fs;

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn set_hook_log_creates_and_deletes_only_the_sentinel() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join(".claudegate");
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::write(home.join("excludes.json"), "{}").unwrap();
    std::env::set_var("CLAUDEGATE_HOME", &home);

    set_hook_log(true).unwrap();
    assert!(home.join("hooklog.enabled").is_file());
    set_hook_log(true).unwrap(); // already there: fine
    assert_eq!(names(&home), vec!["excludes.json", "hooklog.enabled", "sessions"]);
    assert_eq!(fs::read_to_string(home.join("excludes.json")).unwrap(), "{}");

    set_hook_log(false).unwrap();
    assert!(!home.join("hooklog.enabled").exists());
    set_hook_log(false).unwrap(); // already gone: fine
    assert_eq!(names(&home), vec!["excludes.json", "sessions"]);

    // A missing ClaudeGate home is an error, not something to create.
    std::env::set_var("CLAUDEGATE_HOME", t.path().join("nope"));
    assert!(set_hook_log(true).is_err());
    assert!(!t.path().join("nope").exists());
}
