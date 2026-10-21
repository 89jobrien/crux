//! Embeds build provenance so `crux version` can report it and `crux version --check`
//! can detect a binary that has drifted from its source checkout.
//!
//! Emits:
//!   CRUX_GIT_COMMIT  short commit hash of HEAD at build time, or "unknown"
//!   CRUX_GIT_DIRTY   "1" when the tree had uncommitted changes at build time
//!   CRUX_BUILD_EPOCH unix seconds when this build script last ran, or "unknown"
//!   CRUX_PROFILE     cargo profile the binary was built with

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../.git/HEAD");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    // The repository root is two levels up from crates/crux-cli.
    let repo_root = std::path::Path::new(&manifest_dir)
        .ancestors()
        .nth(2)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();

    println!(
        "cargo:rustc-env=CRUX_GIT_COMMIT={}",
        git_output(&repo_root, &["rev-parse", "--short", "HEAD"])
    );
    println!(
        "cargo:rustc-env=CRUX_GIT_DIRTY={}",
        if git_output(&repo_root, &["status", "--porcelain"]).is_empty() {
            "0"
        } else {
            "1"
        }
    );

    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=CRUX_BUILD_EPOCH={epoch}");

    println!(
        "cargo:rustc-env=CRUX_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string())
    );
}

/// Runs git in `dir`, returning trimmed stdout or `"unknown"` on any failure.
///
/// A missing git binary, a non-repository directory, or a non-zero exit all
/// degrade to `"unknown"` so a source tarball without VCS still builds.
fn git_output(dir: &std::path::Path, args: &[&str]) -> String {
    if dir.as_os_str().is_empty() {
        return "unknown".to_string();
    }
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
