//! Drives the real notify watcher on a temp ClaudeGate home. These guard the two ways the live
//! view went wrong: the app's own reads looking like changes (an endless reload loop), and a
//! session file appearing in a sessions/ directory that did not exist when the app started.

use claudegate_desktop_lib::watch::{run, ChangedPayload, KnownMap};
use std::fs;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

/// Everything emitted within `quiet` of the last payload (or `quiet` total if none).
fn drain(rx: &Receiver<ChangedPayload>, quiet: Duration) -> Vec<ChangedPayload> {
    let mut out = vec![];
    let mut deadline = Instant::now() + quiet;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(p) => {
                out.push(p);
                deadline = Instant::now() + quiet;
            }
            Err(_) => break,
        }
    }
    out
}

/// What hook.py and the extension do: write a temp sibling, then rename it over the target.
fn atomic_write(target: &Path, body: &str) {
    let tmp = target.with_file_name(format!(
        "{}.4242.abcdef.tmp",
        target.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&tmp, body).unwrap();
    fs::rename(&tmp, target).unwrap();
}

const QUIET: Duration = Duration::from_millis(800);

#[test]
fn reading_a_session_is_silent_and_an_atomic_write_is_one_payload() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join("aaa.json");
    fs::write(&file, "{}").unwrap();
    let known = KnownMap::default();
    known.lock().unwrap().push(("/ws/a".into(), file.clone()));
    let (tx, rx) = channel();
    run(home.path().to_path_buf(), known, move |p| {
        let _ = tx.send(p);
    });
    // let anything from setup settle
    drain(&rx, QUIET);

    // What list_projects / get_project / get_diff do to the file.
    for _ in 0..3 {
        let _ = fs::read(&file).unwrap();
        let _ = fs::read_to_string(&file).unwrap();
    }
    let _ = fs::metadata(&file).unwrap();
    assert_eq!(drain(&rx, QUIET), vec![], "a read must not look like a change");

    atomic_write(&file, r#"{"files":{}}"#);
    assert_eq!(
        drain(&rx, QUIET),
        vec![ChangedPayload { roots: vec!["/ws/a".into()] }],
        "an atomic rewrite is exactly one change for its root"
    );
}

#[test]
fn a_session_in_a_sessions_dir_created_later_is_one_event() {
    let home = tempfile::tempdir().unwrap();
    let (tx, rx) = channel();
    run(home.path().to_path_buf(), KnownMap::default(), move |p| {
        let _ = tx.send(p);
    });
    drain(&rx, QUIET);

    let sessions = home.path().join("sessions");
    fs::create_dir(&sessions).unwrap();
    atomic_write(&sessions.join("bbb.json"), r#"{"files":{}}"#);
    // Unknown session file and a newly attached sessions/ both mean "re-list everything".
    assert_eq!(drain(&rx, QUIET), vec![ChangedPayload { roots: vec![] }]);

    // and reading it afterwards stays quiet
    let _ = fs::read(sessions.join("bbb.json")).unwrap();
    assert_eq!(drain(&rx, QUIET), vec![]);
}
