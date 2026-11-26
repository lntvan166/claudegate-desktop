use claudegate_core::capture::{app_owns_hook, capture_status, CaptureStatus};

const CMD: &str = "/home/u/.claudegate/hook.sh";

fn settings(matcher: &str, cmd: &str) -> String {
    format!(
        r#"{{"hooks":{{"PreToolUse":[{{"matcher":"{matcher}","hooks":[{{"type":"command","command":"{cmd}"}}]}}]}}}}"#
    )
}

#[test]
fn registered_under_any_matcher_is_extension() {
    for m in ["^(Write|Edit|MultiEdit|Bash)$", "Write|Edit", "^Bash$", "", "*"] {
        assert_eq!(
            capture_status(Some(&settings(m, CMD)), false, true),
            CaptureStatus::SetUpByExtension { python_missing: false },
            "matcher {m:?}"
        );
    }
}

#[test]
fn home_and_windows_forms_are_registered() {
    for c in ["$HOME/.claudegate/hook.sh", "~/.claudegate/hook.sh", "C:\\\\Users\\\\u\\\\.claudegate\\\\hook.bat"] {
        assert_eq!(
            capture_status(Some(&settings("x", c)), false, true),
            CaptureStatus::SetUpByExtension { python_missing: false },
            "{c}"
        );
    }
}

#[test]
fn marker_means_app() {
    assert_eq!(
        capture_status(Some(&settings("x", CMD)), true, true),
        CaptureStatus::SetUpByApp { version: None, python_missing: false }
    );
}

#[test]
fn other_hooks_alongside_ours() {
    let s = r#"{"model":"x","hooks":{"PostToolUse":[{"hooks":[{"command":"/a/.claudegate/hook.sh"}]}],
      "PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"/usr/bin/other"}]},
      {"matcher":"Write","hooks":[{"type":"command","command":"/x"},{"type":"command","command":"/home/u/.claudegate/hook.sh"}]}]}}"#;
    assert_eq!(capture_status(Some(s), false, true), CaptureStatus::SetUpByExtension { python_missing: false });
}

#[test]
fn only_other_hooks_or_wrong_event_is_not_set_up() {
    let s = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"command":"/usr/bin/other"}]}],
      "PostToolUse":[{"hooks":[{"command":"/a/.claudegate/hook.sh"}]}]}}"#;
    assert_eq!(capture_status(Some(s), false, true), CaptureStatus::NotSetUp);
    assert_eq!(capture_status(Some("{}"), true, true), CaptureStatus::NotSetUp);
    assert_eq!(capture_status(Some(r#"{"hooks":{}}"#), false, true), CaptureStatus::NotSetUp);
}

#[test]
fn missing_file_is_not_set_up() {
    assert_eq!(capture_status(None, false, true), CaptureStatus::NotSetUp);
}

#[test]
fn unreadable_settings() {
    for t in ["{ nope", "[]", "42", "null", "{\n// c\n\"hooks\":{}}", "{/* c */}"] {
        assert!(
            matches!(capture_status(Some(t), false, true), CaptureStatus::SettingsUnreadable(_)),
            "{t:?}"
        );
    }
}

#[test]
fn python_missing_blocks_only_when_not_set_up() {
    assert_eq!(capture_status(None, false, false), CaptureStatus::PythonMissing);
    assert_eq!(capture_status(Some("{}"), true, false), CaptureStatus::PythonMissing);
    assert_eq!(capture_status(Some("  "), false, false), CaptureStatus::PythonMissing);
}

#[test]
fn a_registered_hook_wins_over_python_with_a_warning() {
    assert_eq!(
        capture_status(Some(&settings("x", CMD)), false, false),
        CaptureStatus::SetUpByExtension { python_missing: true }
    );
    assert_eq!(
        capture_status(Some(&settings("x", CMD)), true, false),
        CaptureStatus::SetUpByApp { version: None, python_missing: true }
    );
}

#[test]
fn unreadable_settings_win_over_python() {
    // Whether a hook is set up cannot be told, so python3 is not the blocking reason.
    assert!(matches!(capture_status(Some("{ bad"), true, false), CaptureStatus::SettingsUnreadable(_)));
}

#[test]
fn ownership_is_the_stamp() {
    let stamped = b"#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\n";
    assert!(app_owns_hook(true, Some(stamped)));
    assert!(app_owns_hook(true, None), "marker and no hook.py: still the app's setup");
    assert!(!app_owns_hook(true, Some(b"# the extension's hook\n")), "unstamped: the extension replaced it");
    assert!(!app_owns_hook(false, Some(stamped)), "no marker: never the app's");
    assert!(!app_owns_hook(false, None));
}

#[test]
fn unreadable_wins_over_marker() {
    assert!(matches!(capture_status(Some("[]"), true, true), CaptureStatus::SettingsUnreadable(_)));
}

#[test]
fn empty_file_is_not_set_up() {
    assert_eq!(capture_status(Some("  \n"), false, true), CaptureStatus::NotSetUp);
}

#[test]
fn any_claudegate_mention_in_pretooluse_counts() {
    for c in [
        "\\\"/home/u/.claudegate/hook.sh\\\"",
        "/home/u/.claudegate/hook.sh --verbose",
        "/home/u/.claudegate/hook.sh  ",
        "C:/Users/u/.claudegate/hook.bat",
        "python3 ~/.claudegate/hook.py",
    ] {
        assert_eq!(
            capture_status(Some(&settings("x", c)), false, true),
            CaptureStatus::SetUpByExtension { python_missing: false },
            "{c}"
        );
    }
}

#[test]
fn unrelated_hook_is_not_ours() {
    assert_eq!(
        capture_status(Some(&settings("Write", "prettier --write")), false, true),
        CaptureStatus::NotSetUp
    );
}

mod hook_version_states {
    use claudegate_core::capture::{hook_version, HookVersion};

    const BUNDLED: &str = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.17.0\nbody\n";

    #[test]
    fn the_bundled_hook_or_the_extensions_copy_at_its_tag_is_up_to_date() {
        assert_eq!(hook_version(Some(BUNDLED.as_bytes()), BUNDLED), HookVersion::UpToDate);
        assert_eq!(hook_version(Some(b"#!/usr/bin/env python3\nbody\n"), BUNDLED), HookVersion::UpToDate);
    }

    #[test]
    fn an_older_stamp_is_older() {
        let old = b"#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\nbody\n";
        assert_eq!(hook_version(Some(old), BUNDLED), HookVersion::Older { installed: "v1.16.1".into() });
    }

    #[test]
    fn an_unstamped_file_that_differs_is_the_extensions_with_no_version_guessed() {
        assert_eq!(hook_version(Some(b"#!/usr/bin/env python3\nother\n"), BUNDLED), HookVersion::Different { installed: None });
    }

    #[test]
    fn the_same_stamp_with_other_bytes_is_a_modified_copy() {
        let same = b"#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.17.0\nedited\n";
        assert_eq!(hook_version(Some(same), BUNDLED), HookVersion::Different { installed: Some("v1.17.0".into()) });
        let odd = b"#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=nightly\nx\n";
        assert_eq!(hook_version(Some(odd), BUNDLED), HookVersion::Different { installed: Some("nightly".into()) });
    }

    #[test]
    fn a_newer_stamp_is_newer_never_outdated() {
        let newer = b"#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v2.0.0\nx\n";
        assert_eq!(hook_version(Some(newer), BUNDLED), HookVersion::Newer { installed: "v2.0.0".into() });
    }

    #[test]
    fn a_stamp_off_line_two_is_not_a_stamp() {
        let first = b"# CLAUDEGATE_HOOK_VERSION=v1.0.0\n#!/usr/bin/env python3\nx\n";
        assert_eq!(hook_version(Some(first), BUNDLED), HookVersion::Different { installed: None });
        let later = b"#!/usr/bin/env python3\nx\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n";
        assert_eq!(hook_version(Some(later), BUNDLED), HookVersion::Different { installed: None });
    }

    #[test]
    fn no_file_is_missing() {
        assert_eq!(hook_version(None, BUNDLED), HookVersion::Missing);
    }
}
