//! Records the commit and the day the app was built, for About CreaBIM.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    // the build day, as days since 1970, turned into a date by the app
    let day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    println!("cargo:rustc-env=CREABIM_COMMIT={commit}");
    println!("cargo:rustc-env=CREABIM_BUILD_DAY={day}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
}
