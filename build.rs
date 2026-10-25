//! Bakes the short git hash into the binary as `BREAKAWAY_COMMIT` (for
//! playtest results); `unknown` when git is unavailable.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=BREAKAWAY_COMMIT={commit}");
    // Rebuild when HEAD moves: a branch switch, or a commit on the branch.
    // Only existing paths: Cargo reruns on every build for a missing one.
    let mut watch = vec![".git/HEAD".to_owned(), ".git/packed-refs".to_owned()];
    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(r) = head.strip_prefix("ref: ")
    {
        watch.push(format!(".git/{}", r.trim()));
    }
    for path in watch.iter().filter(|p| std::path::Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={path}");
    }
}
