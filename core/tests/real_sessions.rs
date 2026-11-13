//! Read-only check against real session files. Run with
//! `CLAUDEGATE_REAL_SESSIONS=~/.claudegate/sessions cargo test -p claudegate-core --test real_sessions -- --ignored --nocapture`
use claudegate_core::model::{parse_session, Shape};

#[test]
#[ignore]
fn real_sessions_shape() {
    let dir = std::env::var("CLAUDEGATE_REAL_SESSIONS").expect("set CLAUDEGATE_REAL_SESSIONS");
    let (mut total, mut known) = (0, 0);
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        total += 1;
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match parse_session(&std::fs::read_to_string(&path).unwrap()) {
            Ok((_, Shape::Known)) => known += 1,
            Ok((_, Shape::Unknown(p))) => println!("{name}: unknown {p:?}"),
            Err(e) => println!("{name}: error {e}"),
        }
    }
    println!("files={total} known={known}");
}
