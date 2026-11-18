use claudegate_core::capture::{capture_status, CaptureStatus};

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
            CaptureStatus::SetUpByExtension,
            "matcher {m:?}"
        );
    }
}

#[test]
fn home_and_windows_forms_are_registered() {
    for c in ["$HOME/.claudegate/hook.sh", "~/.claudegate/hook.sh", "C:\\\\Users\\\\u\\\\.claudegate\\\\hook.bat"] {
        assert_eq!(
            capture_status(Some(&settings("x", c)), false, true),
            CaptureStatus::SetUpByExtension,
            "{c}"
        );
    }
}

#[test]
fn marker_means_app() {
    assert_eq!(
        capture_status(Some(&settings("x", CMD)), true, true),
        CaptureStatus::SetUpByApp { version: None }
    );
}

#[test]
fn other_hooks_alongside_ours() {
    let s = r#"{"model":"x","hooks":{"PostToolUse":[{"hooks":[{"command":"/a/.claudegate/hook.sh"}]}],
      "PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"/usr/bin/other"}]},
      {"matcher":"Write","hooks":[{"type":"command","command":"/x"},{"type":"command","command":"/home/u/.claudegate/hook.sh"}]}]}}"#;
    assert_eq!(capture_status(Some(s), false, true), CaptureStatus::SetUpByExtension);
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
fn python_missing_wins_over_everything() {
    assert_eq!(capture_status(None, false, false), CaptureStatus::PythonMissing);
    assert_eq!(capture_status(Some("{ bad"), true, false), CaptureStatus::PythonMissing);
    assert_eq!(capture_status(Some(&settings("x", CMD)), true, false), CaptureStatus::PythonMissing);
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
            CaptureStatus::SetUpByExtension,
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
