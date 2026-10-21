//! Integration test: verify budget monotonicity proof using the hooo checker.
//!
//! Gated behind `--features verify` so that ordinary `cargo test` does not
//! pull in the hooo dependency at runtime.

#[cfg(feature = "verify")]
mod budget_proof {
    use std::sync::Arc;

    use hooo::meta_cache::MetaCache;
    use hooo::{Context, Loader, Search};

    /// Returns the path to the `proofs/` directory inside this crate.
    fn proofs_dir() -> String {
        format!("{}/proofs", env!("CARGO_MANIFEST_DIR"))
    }

    /// Initialises a hooo `Loader` for the `proofs/` directory.
    ///
    /// `Loader::new` automatically includes the hooo standard library
    /// (with `pord_transitivity`, `nat`, etc.) and pre-scans every `.hooo`
    /// file in the directory to extract function signatures.
    fn make_loader(meta_cache: &mut MetaCache) -> Loader {
        Loader::new(Arc::new(proofs_dir()), meta_cache, None, None)
            .expect("failed to initialise hooo Loader for proofs/")
    }

    /// Run the hooo proof checker against a single `.hooo` file.
    ///
    /// Returns `Ok(())` when the proof is valid, or an error string
    /// describing the first type-check failure.
    fn check_file(path: &str, loader: &mut Loader, meta_cache: &MetaCache) -> Result<(), String> {
        let mut ctx = Context::new();
        let mut search = Search::new();
        ctx.run(path, &mut search, loader, meta_cache).map(|_| ())
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    /// Verifies that `proofs/budget.hooo` type-checks completely.
    ///
    /// This covers:
    /// - `pord_trans` — transitivity axiom for the partial order
    /// - `consume_mono` — single-step monotonicity axiom for `consume()`
    /// - `step_mono` — single-step monotonicity axiom for `begin_step()`
    /// - `two_consume_mono` — derived two-step monotonicity theorem
    #[test]
    fn budget_monotonicity_proof_verifies() {
        let mut meta_cache = MetaCache::new();
        let mut loader = make_loader(&mut meta_cache);

        let proof_file = format!("{}/budget.hooo", proofs_dir());
        check_file(&proof_file, &mut loader, &meta_cache)
            .unwrap_or_else(|err| panic!("budget.hooo proof failed:\n{err}"));
    }

    /// Smoke-test: `Loader::new` must discover at least one `.hooo` file.
    #[test]
    fn proofs_directory_contains_hooo_files() {
        let mut meta_cache = MetaCache::new();
        let loader = make_loader(&mut meta_cache);
        assert!(
            !loader.files.is_empty(),
            "proofs/ directory contained no .hooo files — check CARGO_MANIFEST_DIR resolution"
        );
    }
}
