use chrono::Utc;

fn main() {
    git_version::set_env();
    println!("cargo:rustc-env=BUILD_DATE={}", Utc::today().naive_utc());
}
