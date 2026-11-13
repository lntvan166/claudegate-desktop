use claudegate_core::project::{list_projects, load_project};
use std::path::PathBuf;

/// Read-only diagnostic; prints counts only, no names or paths.
#[test]
#[ignore]
fn real_projects_summary() {
    let Some(h) = std::env::var_os("CLAUDEGATE_REAL_HOME") else {
        return;
    };
    let home = PathBuf::from(h);
    let (mut projects, mut pending, mut bad) = (0, 0, 0);
    for (i, p) in list_projects(&home).iter().enumerate() {
        let v = load_project(&home, std::path::Path::new(&p.root));
        let b = v.scopes.iter().filter(|s| s.error.is_some() || !s.shape_unknown.is_empty()).count();
        println!("project {i}: missing={} pending={} scopes={} bad_scopes={b}", v.missing, v.pending.len(), v.scopes.len());
        projects += 1;
        pending += v.pending.len();
        bad += b;
    }
    println!("TOTAL projects={projects} pending={pending} bad_scopes={bad}");
}
