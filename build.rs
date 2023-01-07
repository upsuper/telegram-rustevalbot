use std::env;
use std::process::Command;

fn main() {
    println!(
        "cargo:rustc-env=VERSION={} ({})",
        env::var("CARGO_PKG_VERSION").unwrap(),
        get_commit_info().unwrap(),
    );
    println!(
        "cargo:rustc-env=USER_AGENT={} / {} - {}",
        env::var("CARGO_PKG_NAME").unwrap(),
        env::var("CARGO_PKG_VERSION").unwrap(),
        env::var("CARGO_PKG_HOMEPAGE").unwrap(),
    );
}

fn get_commit_info() -> Option<String> {
    let result = Command::new("git")
        .args(["log", "-1", "--date=short", "--pretty=format:%h / %cd"])
        .output()
        .ok()?;
    String::from_utf8(result.stdout).ok()
}
