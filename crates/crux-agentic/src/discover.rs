//! Recursive `.crux` pipeline file discovery.
//!
//! Walks a directory tree respecting `.gitignore` rules and skips
//! hidden directories, `target/`, and `.git/` by default.

use std::path::{Path, PathBuf};

/// Discover all `.crux` pipeline files under `root`, sorted by path.
///
/// Uses gitignore-aware walking (respects `.gitignore` files in the tree).
/// Skips hidden directories, `target/`, and `.git/` automatically.
pub fn discover_pipelines(root: &Path) -> Vec<PathBuf> {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true) // respect .gitignore
        .git_global(false)
        .git_exclude(false)
        .build();

    let mut results: Vec<PathBuf> = walker
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_some_and(|ft| ft.is_file())
                && entry.path().extension().is_some_and(|ext| ext == "crux")
        })
        .map(|entry| entry.into_path())
        .collect();

    results.sort();
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_nested_crux_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create nested .crux files
        fs::write(root.join("top.crux"), "pipeline: top\nsteps: []\n").unwrap();
        fs::create_dir_all(root.join("sub/deep")).unwrap();
        fs::write(root.join("sub/middle.crux"), "pipeline: mid\nsteps: []\n").unwrap();
        fs::write(
            root.join("sub/deep/bottom.crux"),
            "pipeline: bot\nsteps: []\n",
        )
        .unwrap();

        // Non-.crux files should be ignored
        fs::write(root.join("readme.md"), "# hi").unwrap();
        fs::write(root.join("sub/data.json"), "{}").unwrap();

        let found = discover_pipelines(root);
        assert_eq!(found.len(), 3);
        // Sorted by path
        assert!(found[0].ends_with("sub/deep/bottom.crux"));
        assert!(found[1].ends_with("sub/middle.crux"));
        assert!(found[2].ends_with("top.crux"));
    }

    #[test]
    fn respects_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Initialize as git repo so .gitignore is respected
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored/\n").unwrap();

        fs::write(root.join("kept.crux"), "pipeline: k\nsteps: []\n").unwrap();
        fs::create_dir(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/hidden.crux"), "pipeline: h\nsteps: []\n").unwrap();

        let found = discover_pipelines(root);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("kept.crux"));
    }

    #[test]
    fn skips_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("visible.crux"), "pipeline: v\nsteps: []\n").unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/secret.crux"), "pipeline: s\nsteps: []\n").unwrap();

        let found = discover_pipelines(root);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("visible.crux"));
    }

    #[test]
    fn skips_target_dir_via_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // target/ is typically in .gitignore for Rust projects
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();

        fs::write(root.join("app.crux"), "pipeline: a\nsteps: []\n").unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(
            root.join("target/debug/stale.crux"),
            "pipeline: s\nsteps: []\n",
        )
        .unwrap();

        let found = discover_pipelines(root);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("app.crux"));
    }

    #[test]
    fn empty_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let found = discover_pipelines(tmp.path());
        assert!(found.is_empty());
    }
}
