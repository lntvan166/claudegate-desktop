//! Calls the command functions against $CLAUDEGATE_HOME (must be a sandbox) and prints counts.
use claudegate_desktop_lib::commands::{get_project_with, list_projects_with};
use claudegate_desktop_lib::watch::KnownMap;

fn main() {
    let home = claudegate_core::paths::claudegate_home();
    println!("home = {}", home.display());
    let known = KnownMap::default();
    let list = list_projects_with(&home, &known);
    println!("projects = {}", list.len());
    for p in &list {
        let v = get_project_with(&home, &known, &p.root);
        println!(
            "  {} pending(summary)={} pending={} accepted={} rejected={} scopes={}",
            p.name, p.pending, v.pending.len(), v.accepted.len(), v.rejected.len(), v.scopes.len()
        );
    }
    println!("known scope files = {}", known.lock().unwrap().len());
}
