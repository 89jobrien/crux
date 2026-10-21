//! `crux version` — build provenance and source-checkout drift detection.
//!
//! The CLI's `--version` flag prints only the crate version. This module adds
//! the build commit, build time, and profile, plus `--check`, which compares the
//! commit baked into the running binary against the HEAD of a crux source
//! checkout. That catches the common failure mode of an installed `crux` that
//! predates edits in the repo it was installed from.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const UNKNOWN: &str = "unknown";

/// Provenance captured by `build.rs` at compile time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: String,
    pub commit: String,
    pub dirty_at_build: bool,
    pub built_at: Option<String>,
    pub profile: String,
}

impl BuildInfo {
    /// True when a commit was recorded, so drift can actually be assessed.
    pub fn is_verifiable(&self) -> bool {
        self.commit != UNKNOWN
    }
}

fn flag_is_set(value: &str) -> bool {
    value == "1"
}

fn built_at_from_epoch(epoch: &str) -> Option<String> {
    let seconds: i64 = epoch.parse().ok()?;
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Reads the provenance embedded at compile time.
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: env!("CRUX_GIT_COMMIT").to_string(),
        dirty_at_build: flag_is_set(env!("CRUX_GIT_DIRTY")),
        built_at: built_at_from_epoch(env!("CRUX_BUILD_EPOCH")),
        profile: env!("CRUX_PROFILE").to_string(),
    }
}

/// Human-readable provenance block for `crux version`.
pub fn render_build_info(info: &BuildInfo, binary: Option<&Path>) -> String {
    let mut out = format!("crux {}\n", info.version);
    out.push_str(&format!(
        "commit   {}{}\n",
        info.commit,
        if info.dirty_at_build {
            " (dirty tree)"
        } else {
            ""
        }
    ));
    out.push_str(&format!(
        "built    {}\n",
        info.built_at.as_deref().unwrap_or(UNKNOWN)
    ));
    out.push_str(&format!("profile  {}\n", info.profile));
    if let Some(binary) = binary {
        out.push_str(&format!("binary   {}\n", binary.display()));
    }
    out
}

/// Result of comparing the binary's build commit against a source checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Binary commit matches checkout HEAD and the tree is clean.
    UpToDate,
    /// Binary commit matches HEAD, but the working tree has local edits.
    UpToDateWithLocalEdits,
    /// Binary was built from an older commit than the checkout's HEAD.
    Stale { head: String },
    /// Drift could not be determined; `reason` explains why.
    Unverifiable { reason: String },
}

impl Drift {
    /// Process exit code: 0 current, 1 stale, 2 undeterminable.
    pub fn exit_code(&self) -> i32 {
        match self {
            Drift::UpToDate | Drift::UpToDateWithLocalEdits => 0,
            Drift::Stale { .. } => 1,
            Drift::Unverifiable { .. } => 2,
        }
    }
}

/// Compares a built binary against a checkout's HEAD. Pure.
pub fn assess(info: &BuildInfo, head: Option<&str>, tree_dirty: bool) -> Drift {
    if !info.is_verifiable() {
        return Drift::Unverifiable {
            reason: "this binary was built without recorded git provenance".to_string(),
        };
    }
    let Some(head) = head else {
        return Drift::Unverifiable {
            reason: "could not read HEAD from the source checkout".to_string(),
        };
    };
    if head != info.commit {
        return Drift::Stale {
            head: head.to_string(),
        };
    }
    if tree_dirty {
        Drift::UpToDateWithLocalEdits
    } else {
        Drift::UpToDate
    }
}

/// Renders the drift verdict plus remediation hints. Pure.
pub fn render_drift(info: &BuildInfo, checkout: &Path, drift: &Drift) -> String {
    match drift {
        Drift::UpToDate => format!(
            "ok    installed crux matches {} at HEAD\n",
            checkout.display()
        ),
        Drift::UpToDateWithLocalEdits => format!(
            "warn  installed crux matches HEAD, but {} has uncommitted changes\n\
             warn  the binary may predate them\n",
            checkout.display()
        ),
        Drift::Stale { head } => format!(
            "stale installed crux was built from {}\n\
             stale {}/.git/HEAD is at {}\n\
             stale reinstall with: cargo install --path {} --bin crux --force\n",
            info.commit,
            checkout.display(),
            head,
            checkout.join("crates/crux-cli").display()
        ),
        Drift::Unverifiable { reason } => {
            format!("warn  cannot verify installation freshness: {reason}\n")
        }
    }
}

/// Walks up from `start` looking for a crux source checkout.
///
/// A checkout is a directory containing `crates/crux-cli/Cargo.toml`, which
/// distinguishes it from a downstream project that merely depends on crux.
pub fn locate_checkout(start: &Path) -> Option<PathBuf> {
    start
        .canonicalize()
        .unwrap_or_else(|_| start.to_path_buf())
        .ancestors()
        .find(|dir| dir.join("crates/crux-cli/Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

/// Reads `HEAD` from the checkout's git repository. Returns `None` on failure.
pub fn git_head(checkout: &Path) -> Option<String> {
    git(checkout, &["rev-parse", "--short", "HEAD"])
}

/// True when the checkout has uncommitted changes. Returns `false` on failure.
pub fn git_tree_dirty(checkout: &Path) -> bool {
    git(checkout, &["status", "--porcelain"]).is_some_and(|text| !text.trim().is_empty())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
}

/// Entry point for `crux version [--check]`.
pub fn cmd_version(check: bool, path: Option<&str>) {
    let info = build_info();
    let binary = std::env::current_exe().ok();

    if !check {
        print!("{}", render_build_info(&info, binary.as_deref()));
        return;
    }

    let start = match path {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let Some(checkout) = locate_checkout(&start) else {
        eprint!(
            "{}",
            render_drift(
                &info,
                &start,
                &Drift::Unverifiable {
                    reason: format!("{} is not inside a crux source checkout", start.display()),
                },
            )
        );
        std::process::exit(2);
    };

    let drift = assess(
        &info,
        git_head(&checkout).as_deref(),
        git_tree_dirty(&checkout),
    );
    eprint!("{}", render_drift(&info, &checkout, &drift));
    std::process::exit(drift.exit_code());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(commit: &str) -> BuildInfo {
        BuildInfo {
            version: "0.4.0".to_string(),
            commit: commit.to_string(),
            dirty_at_build: false,
            built_at: Some("2026-10-16T12:00:00Z".to_string()),
            profile: "release".to_string(),
        }
    }

    #[test]
    fn build_info_matches_embedded_provenance() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.profile.is_empty());
        assert_eq!(info.is_verifiable(), info.commit != UNKNOWN);
    }

    #[test]
    fn build_info_render_lists_every_field() {
        let rendered = render_build_info(&info("abc1234"), Some(Path::new("/usr/bin/crux")));
        for expected in [
            "crux 0.4.0",
            "commit   abc1234",
            "built    2026-10-16T12:00:00Z",
            "profile  release",
            "binary   /usr/bin/crux",
        ] {
            assert!(rendered.contains(expected), "{rendered}");
        }
        assert!(!rendered.contains("dirty tree"), "{rendered}");
    }

    #[test]
    fn dirty_build_is_labelled() {
        let mut built = info("abc1234");
        built.dirty_at_build = true;
        assert!(render_build_info(&built, None).contains("abc1234 (dirty tree)"));
    }

    #[test]
    fn epoch_is_formatted_as_rfc3339() {
        assert_eq!(
            built_at_from_epoch("1760000000").as_deref(),
            Some("2025-10-09T08:53:20Z")
        );
        assert_eq!(built_at_from_epoch("unknown"), None);
        assert_eq!(built_at_from_epoch("nonsense"), None);
    }

    #[test]
    fn matching_commit_is_up_to_date() {
        assert_eq!(
            assess(&info("abc1234"), Some("abc1234"), false),
            Drift::UpToDate
        );
        assert_eq!(Drift::UpToDate.exit_code(), 0);
    }

    #[test]
    fn matching_commit_with_local_edits_warns() {
        assert_eq!(
            assess(&info("abc1234"), Some("abc1234"), true),
            Drift::UpToDateWithLocalEdits
        );
    }

    #[test]
    fn older_commit_is_stale() {
        assert_eq!(
            assess(&info("aaa1111"), Some("bbb2222"), false),
            Drift::Stale {
                head: "bbb2222".to_string()
            }
        );
        assert_eq!(Drift::Stale { head: "x".into() }.exit_code(), 1);
    }

    #[test]
    fn missing_provenance_or_head_is_unverifiable() {
        assert_eq!(
            assess(&info(UNKNOWN), Some("abc1234"), false),
            Drift::Unverifiable {
                reason: "this binary was built without recorded git provenance".to_string()
            }
        );
        assert!(matches!(
            assess(&info("abc1234"), None, false),
            Drift::Unverifiable { .. }
        ));
        assert_eq!(assess(&info("abc1234"), None, false).exit_code(), 2);
    }

    #[test]
    fn drift_rendering_carries_remediation_for_stale_binaries() {
        let stale = render_drift(
            &info("aaa1111"),
            Path::new("/src/crux"),
            &Drift::Stale {
                head: "bbb2222".to_string(),
            },
        );
        assert!(stale.contains("stale"), "{stale}");
        assert!(stale.contains("bbb2222"), "{stale}");
        assert!(stale.contains("cargo install --path"), "{stale}");

        assert!(
            render_drift(&info("a"), Path::new("/src/crux"), &Drift::UpToDate).starts_with("ok")
        );
        assert!(
            render_drift(&info("a"), Path::new("/s"), &Drift::UpToDateWithLocalEdits)
                .contains("warn")
        );
        assert!(
            render_drift(
                &info("a"),
                Path::new("/s"),
                &Drift::Unverifiable {
                    reason: "why".into()
                }
            )
            .contains("why")
        );
    }

    #[test]
    fn locate_checkout_requires_a_crux_source_tree() {
        let workspace = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(workspace.path().join("crates/crux-cli")).expect("create dirs");
        std::fs::write(workspace.path().join("crates/crux-cli/Cargo.toml"), "")
            .expect("write manifest");

        let nested = workspace.path().join("deep/nested/dir");
        std::fs::create_dir_all(&nested).expect("create nested");
        assert_eq!(
            locate_checkout(&nested).and_then(|p| p.canonicalize().ok()),
            workspace.path().canonicalize().ok()
        );

        let outside = tempfile::tempdir().expect("tempdir");
        assert_eq!(locate_checkout(outside.path()), None);
    }
}
