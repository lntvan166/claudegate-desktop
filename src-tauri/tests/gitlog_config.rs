//! `gitLog` in config.json: ticks, profiles, discovery settings, panes. Temp config.json only.
//! Every writer adds, removes or sets one key; none overwrites a list with the UI's copy.
use claudegate_desktop_lib::config::{
    config_path_in, gitlog_apply_profile, gitlog_delete_profile, gitlog_exclude_repo, gitlog_include_repo, gitlog_rename_profile,
    gitlog_save_profile, gitlog_set_blame, gitlog_set_last_filter, gitlog_set_pane, gitlog_set_scan_depth, gitlog_tick_all, gitlog_tick_only,
    gitlog_tick_repos, gitlog_untick_repos, read_gitlog, GITLOG_SCAN_DEPTH, FavoriteView, PairView, gitlog_favorite_branch, gitlog_push_recent_pair,
    gitlog_set_compare_mode, gitlog_set_compare_pair, gitlog_unfavorite_branch,
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

fn pv(l: &str, lr: bool, r: &str, rr: bool) -> PairView {
    PairView { left: l.into(), left_remote: lr, right: r.into(), right_remote: rr }
}

#[test]
fn compare_pair_is_one_key() {
    let (_t, p) = cfg();
    let before = json!({"theme": "dark", "gitLog": {"ticked": ["a"], "panes": {"repos": 200}, "blame": false, "futureKey": [1, 2]}});
    fs::write(&p, before.to_string()).unwrap();
    let g = gitlog_set_compare_pair(&p, "release-1.4", false, "origin/main", true, None).unwrap();
    assert_eq!(g.compare_pair, Some(pv("release-1.4", false, "origin/main", true)));
    let mut after = raw(&p);
    assert_eq!(after["gitLog"]["comparePair"], json!({"left": "release-1.4", "leftRemote": false, "right": "origin/main", "rightRemote": true}));
    after["gitLog"].as_object_mut().unwrap().remove("comparePair");
    assert_eq!(after, before, "every other key is byte-identical");
    let g = gitlog_set_compare_pair(&p, "main", false, "feat/billing", false, None).unwrap();
    assert_eq!(g.compare_pair, Some(pv("main", false, "feat/billing", false)));
    let mut again = raw(&p);
    again["gitLog"].as_object_mut().unwrap().remove("comparePair");
    assert_eq!(again, before);
    // A malformed stored pair (no kind, bad name) reads as absent.
    fs::write(&p, json!({"gitLog": {"comparePair": {"left": "a", "right": "b"}}}).to_string()).unwrap();
    assert_eq!(read_gitlog(&p).compare_pair, None);
    fs::write(&p, json!({"gitLog": {"comparePair": {"left": "-x", "leftRemote": false, "right": "b", "rightRemote": false}}}).to_string()).unwrap();
    assert_eq!(read_gitlog(&p).compare_pair, None);
}

#[test]
fn recent_pairs_front_dedupe_cap5() {
    let (_t, p) = cfg();
    let (p1, p2) = (pv("a", false, "b", false), pv("c", false, "d", true));
    assert_eq!(gitlog_push_recent_pair(&p, "a", false, "b", false, None).unwrap().compare_recent, vec![p1.clone()]);
    assert_eq!(gitlog_push_recent_pair(&p, "c", false, "d", true, None).unwrap().compare_recent, vec![p2.clone(), p1.clone()]);
    // Picking p1 again moves it to the front; the same names with another kind are another pair.
    assert_eq!(gitlog_push_recent_pair(&p, "a", false, "b", false, None).unwrap().compare_recent, vec![p1.clone(), p2.clone()]);
    let g = gitlog_push_recent_pair(&p, "a", false, "b", true, None).unwrap();
    assert_eq!(g.compare_recent, vec![pv("a", false, "b", true), p1.clone(), p2.clone()]);
    // Pushing the front pair again writes nothing.
    let bytes = fs::read(&p).unwrap();
    gitlog_push_recent_pair(&p, "a", false, "b", true, None).unwrap();
    assert_eq!(fs::read(&p).unwrap(), bytes);
    for i in 0..9 {
        gitlog_push_recent_pair(&p, &format!("l{i}"), false, "main", false, None).unwrap();
    }
    let g = read_gitlog(&p);
    assert_eq!(g.compare_recent.len(), 5);
    assert_eq!(g.compare_recent[0], pv("l8", false, "main", false));
    assert_eq!(g.compare_recent[4], pv("l4", false, "main", false));
}

#[test]
fn favorites_add_and_remove_only() {
    let (_t, p) = cfg();
    gitlog_favorite_branch(&p, "main", false, None).unwrap();
    let g = gitlog_favorite_branch(&p, "main", false, None).unwrap();
    assert_eq!(g.compare_favorites, vec![FavoriteView { name: "main".into(), remote: false }], "twice keeps one");
    let g = gitlog_favorite_branch(&p, "main", true, None).unwrap();
    assert_eq!(g.compare_favorites.len(), 2, "the remote main is another branch");
    // A second window added one meanwhile: removing touches only the named one.
    let mut v = raw(&p);
    v["gitLog"]["compareFavorites"].as_array_mut().unwrap().push(json!({"name": "dev", "remote": false}));
    fs::write(&p, v.to_string()).unwrap();
    let g = gitlog_unfavorite_branch(&p, "main", true, None).unwrap();
    assert_eq!(g.compare_favorites, vec![FavoriteView { name: "main".into(), remote: false }, FavoriteView { name: "dev".into(), remote: false }]);
    let bytes = fs::read(&p).unwrap();
    gitlog_unfavorite_branch(&p, "gone", false, None).unwrap();
    assert_eq!(fs::read(&p).unwrap(), bytes, "removing a missing name writes nothing");
}

#[test]
fn compare_mode_validates() {
    let (_t, p) = cfg();
    assert_eq!(read_gitlog(&p).compare_mode, "files");
    assert_eq!(gitlog_set_compare_mode(&p, "commits", None).unwrap().compare_mode, "commits");
    assert_eq!(gitlog_set_compare_mode(&p, "files", None).unwrap().compare_mode, "files");
    assert_eq!(gitlog_set_compare_mode(&p, "other", None), Err("Unknown mode.".to_string()));
    fs::write(&p, json!({"gitLog": {"compareMode": 3}}).to_string()).unwrap();
    assert_eq!(read_gitlog(&p).compare_mode, "files");
}

#[test]
fn invalid_branch_names_refused() {
    let (_t, p) = cfg();
    let no = Err("Not a branch name.".to_string());
    for bad in ["-x", "a..b", "x y"] {
        assert_eq!(gitlog_set_compare_pair(&p, bad, false, "main", false, None), no, "{bad:?}");
        assert_eq!(gitlog_set_compare_pair(&p, "main", false, bad, true, None), no, "{bad:?}");
        assert_eq!(gitlog_push_recent_pair(&p, bad, false, "main", false, None), no, "{bad:?}");
        assert_eq!(gitlog_favorite_branch(&p, bad, false, None), no, "{bad:?}");
        assert_eq!(gitlog_unfavorite_branch(&p, bad, false, None), no, "{bad:?}");
    }
    assert!(!p.exists(), "nothing was written");
}
