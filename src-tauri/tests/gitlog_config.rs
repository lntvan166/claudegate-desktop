//! `gitLog` in config.json: ticks, profiles, discovery settings, panes. Temp config.json only.
//! Every writer adds, removes or sets one key; none overwrites a list with the UI's copy.
use claudegate_desktop_lib::config::{
    config_path_in, gitlog_apply_profile, gitlog_delete_profile, gitlog_exclude_repo, gitlog_include_repo, gitlog_rename_profile,
    gitlog_save_profile, gitlog_set_blame, gitlog_set_last_filter, gitlog_set_pane, gitlog_set_scan_depth, gitlog_tick_all, gitlog_tick_only,
    gitlog_tick_repos, gitlog_untick_repos, read_gitlog, GITLOG_SCAN_DEPTH,
};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn cfg() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = config_path_in(t.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    (t, p)
}

fn raw(p: &PathBuf) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

const KNOWN: [&str; 3] = ["/w/acme-api/.git", "/w/acme-web/.git", "/w/svc-lib/.git"];
fn known() -> Result<Vec<String>, String> {
    Ok(ids(&KNOWN))
}

#[test]
fn defaults_when_absent_or_malformed() {
    let (_t, p) = cfg();
    let g = read_gitlog(&p);
    assert_eq!(g.ticked, None);
    assert_eq!(g.scan_depth, GITLOG_SCAN_DEPTH);
    assert!(g.profiles.is_empty() && g.exclude_repos.is_empty() && g.panes.is_empty());
    fs::write(&p, json!({"gitLog": {"ticked": 3, "profiles": [1], "scanDepth": 40, "panes": {"repos": -1, "list": 300}}}).to_string()).unwrap();
    let g = read_gitlog(&p);
    assert_eq!(g.ticked, None, "a ticked list of the wrong shape reads as every repo");
    assert_eq!(g.scan_depth, 8, "capped like workspace_scan");
    assert!(g.profiles.is_empty());
    assert_eq!(g.panes.len(), 1);
}

#[test]
fn untick_from_all_uses_the_backend_list_and_tick_is_add_only() {
    let (_t, p) = cfg();
    // Every repo ticked: ticking writes nothing.
    gitlog_tick_repos(&p, &ids(&[KNOWN[0]]), None).unwrap();
    assert!(!p.exists());
    let g = gitlog_untick_repos(&p, &ids(&[KNOWN[1]]), known, None).unwrap();
    assert_eq!(g.ticked, Some(ids(&[KNOWN[0], KNOWN[2]])));
    // A second window changed the file meanwhile; a tick adds to that, never replaces it.
    let mut v = raw(&p);
    v["gitLog"]["ticked"] = json!([KNOWN[0]]);
    fs::write(&p, v.to_string()).unwrap();
    let g = gitlog_tick_repos(&p, &ids(&[KNOWN[1]]), None).unwrap();
    assert_eq!(g.ticked, Some(ids(&[KNOWN[0], KNOWN[1]])));
    // Untick is remove-only on an explicit list: `known` is not consulted.
    let g = gitlog_untick_repos(&p, &ids(&[KNOWN[0]]), || panic!("not needed"), None).unwrap();
    assert_eq!(g.ticked, Some(ids(&[KNOWN[1]])));
    let g = gitlog_tick_all(&p, None).unwrap();
    assert_eq!(g.ticked, None);
    assert!(raw(&p)["gitLog"].get("ticked").is_none());
}

#[test]
fn untick_before_discovery_fails_without_writing() {
    let (_t, p) = cfg();
    let e = gitlog_untick_repos(&p, &ids(&[KNOWN[0]]), || Err("not loaded".into()), None).unwrap_err();
    assert_eq!(e, "not loaded");
    assert!(!p.exists());
}

#[test]
fn other_keys_survive() {
    let (_t, p) = cfg();
    fs::write(&p, json!({"readOnly": true, "workspace": ["/w"], "gitLog": {"lastFilter": {"search": "x"}}}).to_string()).unwrap();
    gitlog_tick_only(&p, &ids(&[KNOWN[2]]), None).unwrap();
    gitlog_set_scan_depth(&p, 3, None).unwrap();
    let v = raw(&p);
    assert_eq!(v["readOnly"], json!(true));
    assert_eq!(v["workspace"], json!(["/w"]));
    assert_eq!(v["gitLog"]["lastFilter"], json!({"search": "x"}));
    assert_eq!(v["gitLog"]["ticked"], json!([KNOWN[2]]));
    assert_eq!(v["gitLog"]["scanDepth"], json!(3));
    assert!(gitlog_tick_only(&p, &[], None).is_err());
    assert!(gitlog_set_scan_depth(&p, 9, None).is_err());
}

#[test]
fn profiles_save_apply_rename_delete() {
    let (_t, p) = cfg();
    // Saved while every repo is ticked: the discovered list.
    let g = gitlog_save_profile(&p, " Everything ", known, None).unwrap();
    assert_eq!(g.profiles["Everything"], ids(&KNOWN));
    gitlog_tick_only(&p, &ids(&[KNOWN[0], "/gone/.git"]), None).unwrap();
    gitlog_save_profile(&p, "api", || panic!("explicit ticks need no list"), None).unwrap();
    gitlog_set_last_filter(&p, json!({"search": "fix"}), None).unwrap();
    gitlog_tick_all(&p, None).unwrap();
    // Applying copies the stored profile, missing ids included; the filter is left alone.
    let g = gitlog_apply_profile(&p, "api", None).unwrap();
    assert_eq!(g.ticked, Some(ids(&[KNOWN[0], "/gone/.git"])));
    assert_eq!(g.last_filter, Some(json!({"search": "fix"})));
    assert!(gitlog_apply_profile(&p, "nope", None).is_err());
    assert!(gitlog_rename_profile(&p, "api", "Everything", None).is_err(), "never overwrites another profile");
    let g = gitlog_rename_profile(&p, "api", "API only", None).unwrap();
    assert!(g.profiles.contains_key("API only") && !g.profiles.contains_key("api"));
    let g = gitlog_delete_profile(&p, "API only", None).unwrap();
    assert_eq!(g.profiles.keys().collect::<Vec<_>>(), vec!["Everything"]);
    assert!(gitlog_save_profile(&p, "  ", known, None).is_err());
}

#[test]
fn exclude_is_add_and_remove_only() {
    let (_t, p) = cfg();
    gitlog_exclude_repo(&p, "**/vendor-mirror", None).unwrap();
    // Another writer added one meanwhile.
    let mut v = raw(&p);
    v["gitLog"]["excludeRepos"] = json!(["**/vendor-mirror", "scratch"]);
    fs::write(&p, v.to_string()).unwrap();
    let g = gitlog_exclude_repo(&p, "old-*", None).unwrap();
    assert_eq!(g.exclude_repos, ids(&["**/vendor-mirror", "scratch", "old-*"]));
    let g = gitlog_include_repo(&p, "scratch", None).unwrap();
    assert_eq!(g.exclude_repos, ids(&["**/vendor-mirror", "old-*"]));
    assert!(gitlog_exclude_repo(&p, " ", None).is_err());
}

#[test]
fn pane_sets_one_width() {
    let (_t, p) = cfg();
    gitlog_set_pane(&p, "repos", 240.4, None).unwrap();
    let g = gitlog_set_pane(&p, "changes", 300.0, None).unwrap();
    assert_eq!(g.panes["repos"], 240.0);
    assert_eq!(g.panes["changes"], 300.0);
    assert!(gitlog_set_pane(&p, "repos", f64::NAN, None).is_err());
    assert!(gitlog_set_pane(&p, "../x", 10.0, None).is_err());
}

#[test]
fn refuses_inside_the_claudegate_home() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("config.json");
    assert!(gitlog_tick_all(&p, Some(t.path())).is_err());
}

#[test]
fn blame_setting_is_one_key() {
    let (_t, p) = cfg();
    assert!(read_gitlog(&p).blame, "on by default");
    let before = json!({"theme": "dark", "gitLog": {"ticked": ["a"], "panes": {"repos": 200}, "scanDepth": 3}});
    fs::write(&p, before.to_string()).unwrap();
    let g = gitlog_set_blame(&p, false, None).unwrap();
    assert!(!g.blame);
    let mut after = raw(&p);
    assert_eq!(after["gitLog"]["blame"], json!(false));
    after["gitLog"].as_object_mut().unwrap().remove("blame");
    assert_eq!(after, before, "every other key is untouched");
    assert!(gitlog_set_blame(&p, true, None).unwrap().blame);
    fs::write(&p, json!({"gitLog": {"blame": "yes"}}).to_string()).unwrap();
    assert!(read_gitlog(&p).blame, "a non-bool reads as on");
    fs::write(&p, json!({"gitLog": {"blame": 0}}).to_string()).unwrap();
    assert!(read_gitlog(&p).blame);
}
