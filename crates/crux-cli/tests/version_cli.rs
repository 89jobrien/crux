use std::process::Command;

fn crux() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crux"))
}

#[test]
fn root_version_flag_prints_the_crate_version() {
    for flag in ["--version", "-V"] {
        let output = crux().arg(flag).output().expect("crux --version");
        assert!(output.status.success(), "{flag}");
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert!(
            rendered.starts_with("crux ") && rendered.trim() != "crux",
            "{flag}: {rendered}"
        );
    }
}

#[test]
fn version_flag_is_propagated_to_subcommands() {
    for subcommand in ["run", "check", "schema", "plan", "version"] {
        let output = crux()
            .args([subcommand, "--version"])
            .output()
            .expect("subcommand --version");
        assert!(
            output.status.success(),
            "{subcommand}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert!(
            rendered.contains(env!("CARGO_PKG_VERSION")),
            "{subcommand}: {rendered}"
        );
    }
}

#[test]
fn version_reports_provenance_and_binary_location() {
    let output = crux().arg("version").output().expect("crux version");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = String::from_utf8(output.stdout).unwrap();
    for field in ["commit", "built", "profile", "binary"] {
        assert!(rendered.contains(field), "{rendered}");
    }
    assert!(rendered.contains("crux "), "{rendered}");
}

#[test]
fn version_check_resolves_inside_the_crux_checkout() {
    // CARGO_MANIFEST_DIR is crates/crux-cli, so this is a real source checkout.
    let output = crux()
        .args(["version", "--check"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("crux version --check");
    let code = output.status.code().expect("exit code");
    // 0 = current, 1 = stale, 2 = unverifiable. Never 2 inside the checkout.
    assert_ne!(code, 2, "{}", String::from_utf8_lossy(&output.stderr));
    assert!(code == 0 || code == 1, "unexpected exit code {code}");
}

#[test]
fn version_check_fails_outside_a_crux_checkout() {
    let outside = tempfile::tempdir().expect("tempdir");
    let output = crux()
        .args(["version", "--check"])
        .current_dir(outside.path())
        .output()
        .expect("crux version --check");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not inside a crux source checkout"),
        "{stderr}"
    );
}

#[test]
fn version_check_honors_an_explicit_path() {
    let output = crux()
        .args(["version", "--check", "--path", env!("CARGO_MANIFEST_DIR")])
        .current_dir(std::env::temp_dir())
        .output()
        .expect("crux version --check --path");
    assert_ne!(output.status.code(), Some(2));
}
