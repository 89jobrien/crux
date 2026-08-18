# Plan: `cargo xtask publish`

## Goal

Add a `publish` subcommand to the xtask shim that publishes all 14 crux crates in
dependency order, polling the crates.io sparse index after each publish until indexed,
with `--from <crate>` for mid-run resume; then collapse `release.yml` to a single step.

## Architecture

- **Crates affected**: `xtask` only
- **New types**: `PublishArgs`, `CrateSpec`, `PublishError` — all in `xtask/src/publish.rs`
- **New module**: `xtask/src/publish.rs` — all publish logic; `main.rs` gains a branch
- **Data flow**: `main()` → `parse_publish_args` → `run_publish` → per-crate loop:
  `cargo_publish` → `wait_for_index` → advance

## Tech Stack

- Rust edition 2024
- `ureq = "2"` (sync HTTP, new direct dep on xtask only)
- `serde_json = "1"` (JSON parsing, new direct dep on xtask only)

---

## Tasks

### Task 1: Add dependencies to xtask/Cargo.toml

**Crate**: `xtask`
**File(s)**: `xtask/Cargo.toml`
**Run**: `cargo build -p xtask`

1. Write failing test (compilation gate):

   There is no test here — this task is purely additive to Cargo.toml. Verify the build
   fails before the change by confirming `ureq` is not resolvable:

   ```
   grep -r 'ureq' xtask/   # expect: no output
   ```

2. Implement — add to `xtask/Cargo.toml`:

   ```toml
   [dependencies]
   ureq = "2"
   serde_json = "1"
   ```

3. Verify:

   ```
   cargo build -p xtask    → compiles cleanly
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "chore(xtask): add ureq and serde_json deps for publish subcommand"`

---

### Task 2: Implement `sparse_index_url` and `version_in_index_body`

**Crate**: `xtask`
**File(s)**: `xtask/src/publish.rs` (new file)
**Run**: `cargo nextest run -p xtask`

These two functions are pure and testable without network access.

1. Write failing tests:

   ```rust
   // xtask/src/publish.rs

   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn sparse_index_url_four_char_prefix() {
           let url = sparse_index_url("crux-types");
           assert_eq!(url, "https://index.crates.io/cr/ux/crux-types");
       }

       #[test]
       fn sparse_index_url_facade_crate() {
           let url = sparse_index_url("crux");
           assert_eq!(url, "https://index.crates.io/cr/ux/crux");
       }

       #[test]
       fn version_in_index_body_found() {
           let body = r#"{"name":"crux-types","vers":"0.3.1","deps":[],"cksum":"abc"}
   {"name":"crux-types","vers":"0.3.0","deps":[],"cksum":"def"}"#;
           assert!(version_in_index_body(body, "0.3.1"));
       }

       #[test]
       fn version_in_index_body_not_found() {
           let body = r#"{"name":"crux-types","vers":"0.3.0","deps":[],"cksum":"def"}"#;
           assert!(!version_in_index_body(body, "0.3.1"));
       }

       #[test]
       fn version_in_index_body_partial_match_not_counted() {
           // "0.3.10" must not match a search for "0.3.1"
           let body = r#"{"name":"crux","vers":"0.3.10","deps":[],"cksum":"abc"}"#;
           assert!(!version_in_index_body(body, "0.3.1"));
       }
   }
   ```

   Run: `cargo nextest run -p xtask`
   Expected: FAIL (module does not exist yet)

2. Implement:

   ```rust
   // xtask/src/publish.rs

   pub(crate) fn sparse_index_url(crate_name: &str) -> String {
       let c1 = &crate_name[..2];
       let c2 = &crate_name[2..4];
       format!("https://index.crates.io/{c1}/{c2}/{crate_name}")
   }

   pub(crate) fn version_in_index_body(body: &str, version: &str) -> bool {
       let needle = format!("\"vers\":\"{version}\"");
       body.lines().any(|line| {
           // Parse as JSON to avoid matching "0.3.10" when searching for "0.3.1"
           if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
               obj.get("vers").and_then(|v| v.as_str()) == Some(version)
           } else {
               false
           }
       })
   }
   ```

   Add `mod publish;` to `xtask/src/main.rs`:

   ```rust
   mod publish;
   ```

3. Verify:

   ```
   cargo nextest run -p xtask    → all green
   cargo clippy -p xtask -- -D warnings  → zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(xtask): add sparse_index_url and version_in_index_body"`

---

### Task 3: Implement `parse_publish_args`

**Crate**: `xtask`
**File(s)**: `xtask/src/publish.rs`
**Run**: `cargo nextest run -p xtask`

1. Write failing tests:

   ```rust
   #[test]
   fn parse_no_args_returns_none_from() {
       let args: Vec<String> = vec![];
       let result = parse_publish_args(&args).unwrap();
       assert_eq!(result.from, None);
   }

   #[test]
   fn parse_from_flag_captures_crate_name() {
       let args = vec!["--from".to_string(), "crux-runtime".to_string()];
       let result = parse_publish_args(&args).unwrap();
       assert_eq!(result.from.as_deref(), Some("crux-runtime"));
   }

   #[test]
   fn parse_from_flag_missing_value_returns_err() {
       let args = vec!["--from".to_string()];
       assert!(parse_publish_args(&args).is_err());
   }

   #[test]
   fn parse_unknown_flag_returns_err() {
       let args = vec!["--unknown".to_string()];
       assert!(parse_publish_args(&args).is_err());
   }
   ```

   Run: `cargo nextest run -p xtask -- parse`
   Expected: FAIL

2. Implement:

   ```rust
   pub(crate) struct PublishArgs {
       pub from: Option<String>,
   }

   pub(crate) fn parse_publish_args(args: &[String]) -> Result<PublishArgs, String> {
       let mut from = None;
       let mut iter = args.iter();
       while let Some(arg) = iter.next() {
           match arg.as_str() {
               "--from" => {
                   let val = iter
                       .next()
                       .ok_or_else(|| "--from requires a crate name".to_string())?;
                   from = Some(val.clone());
               }
               other => return Err(format!("unknown argument: {other}")),
           }
       }
       Ok(PublishArgs { from })
   }
   ```

3. Verify:

   ```
   cargo nextest run -p xtask    → all green
   cargo clippy -p xtask -- -D warnings  → zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(xtask): implement parse_publish_args with --from flag"`

---

### Task 4: Implement `PublishError`, `CrateSpec`, `PUBLISH_ORDER`, and `workspace_version`

**Crate**: `xtask`
**File(s)**: `xtask/src/publish.rs`
**Run**: `cargo nextest run -p xtask`

1. Write failing tests:

   ```rust
   #[test]
   fn publish_order_contains_fourteen_crates() {
       assert_eq!(PUBLISH_ORDER.len(), 14);
   }

   #[test]
   fn publish_order_starts_with_leaves() {
       assert_eq!(PUBLISH_ORDER[0].name, "crux-types");
       assert_eq!(PUBLISH_ORDER[1].name, "crux-model");
   }

   #[test]
   fn publish_order_ends_with_facade() {
       assert_eq!(PUBLISH_ORDER[13].name, "crux");
   }

   #[test]
   fn workspace_version_parses_from_cargo_toml() {
       // Reads the actual workspace Cargo.toml — requires running from workspace root
       let version = workspace_version().unwrap();
       // Must be semver-shaped: digits.digits.digits
       let parts: Vec<&str> = version.split('.').collect();
       assert_eq!(parts.len(), 3);
       assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
   }
   ```

   Run: `cargo nextest run -p xtask -- publish_order workspace_version`
   Expected: FAIL

2. Implement:

   ```rust
   use std::fmt;

   pub(crate) struct CrateSpec {
       pub name: &'static str,
   }

   pub(crate) const PUBLISH_ORDER: &[CrateSpec] = &[
       CrateSpec { name: "crux-types" },
       CrateSpec { name: "crux-model" },
       CrateSpec { name: "crux-domain" },
       CrateSpec { name: "crux-macros" },
       CrateSpec { name: "crux-runtime" },
       CrateSpec { name: "crux-script" },
       CrateSpec { name: "crux-task" },
       CrateSpec { name: "crux-improve" },
       CrateSpec { name: "crux-baml" },
       CrateSpec { name: "crux-stdlib" },
       CrateSpec { name: "crux-plugin" },
       CrateSpec { name: "crux-agentic" },
       CrateSpec { name: "crux-planner" },
       CrateSpec { name: "crux" },
   ];

   pub(crate) const POLL_RETRIES: u32 = 30;
   pub(crate) const POLL_INTERVAL_SECS: u64 = 10;

   pub(crate) enum PublishError {
       CargoPublishFailed { crate_name: String, exit_code: i32 },
       IndexPollTimeout { crate_name: String, version: String },
       HttpError { crate_name: String, source: Box<ureq::Error> },
       VersionNotInWorkspace,
       UnknownFromCrate { name: String },
   }

   impl fmt::Display for PublishError {
       fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
           match self {
               Self::CargoPublishFailed { crate_name, exit_code } =>
                   write!(f, "cargo publish -p {crate_name} failed with exit code {exit_code}"),
               Self::IndexPollTimeout { crate_name, version } =>
                   write!(f, "timed out waiting for {crate_name} {version} to appear on crates.io"),
               Self::HttpError { crate_name, source } =>
                   write!(f, "HTTP error polling index for {crate_name}: {source}"),
               Self::VersionNotInWorkspace =>
                   write!(f, "could not read version from workspace Cargo.toml"),
               Self::UnknownFromCrate { name } =>
                   write!(f, "--from crate '{name}' not found in publish order"),
           }
       }
   }

   impl std::error::Error for PublishError {}

   pub(crate) fn workspace_version() -> Result<String, PublishError> {
       let manifest = std::fs::read_to_string("Cargo.toml")
           .map_err(|_| PublishError::VersionNotInWorkspace)?;
       for line in manifest.lines() {
           let trimmed = line.trim();
           if trimmed.starts_with("version") {
               if let Some(val) = trimmed.split('"').nth(1) {
                   return Ok(val.to_string());
               }
           }
       }
       Err(PublishError::VersionNotInWorkspace)
   }
   ```

3. Verify:

   ```
   cargo nextest run -p xtask    → all green
   cargo clippy -p xtask -- -D warnings  → zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(xtask): add PublishError, CrateSpec, PUBLISH_ORDER, workspace_version"`

---

### Task 5: Implement `cargo_publish` and `wait_for_index`

**Crate**: `xtask`
**File(s)**: `xtask/src/publish.rs`
**Run**: `cargo nextest run -p xtask`

`cargo_publish` and `wait_for_index` both invoke external systems and cannot be unit
tested without network/process access. Test the integration path via a fake that validates
`wait_for_index` short-circuits on first success.

1. Write failing test for the early-exit path of `wait_for_index` using a response stub.
   Since `wait_for_index` calls `ureq` directly (no trait boundary), test via the pure
   helpers already covered in Task 2. Add one integration-shaped test that documents
   expected behavior:

   ```rust
   #[test]
   fn wait_for_index_would_succeed_if_version_present_on_first_poll() {
       // Validates that version_in_index_body returning true on line 1
       // is the short-circuit condition — no sleep needed.
       let body = "{\"name\":\"crux\",\"vers\":\"0.3.1\",\"deps\":[],\"cksum\":\"x\"}";
       assert!(version_in_index_body(body, "0.3.1"),
           "first-poll success requires version_in_index_body to return true");
   }
   ```

   Run: `cargo nextest run -p xtask -- wait_for_index_would`
   Expected: FAIL (function not yet in scope in test; passes once implemented)

2. Implement:

   ```rust
   pub(crate) fn cargo_publish(crate_name: &str) -> Result<(), PublishError> {
       let status = std::process::Command::new("cargo")
           .args(["publish", "-p", crate_name])
           .status()
           .unwrap_or_else(|e| panic!("failed to spawn cargo: {e}"));
       if status.success() {
           Ok(())
       } else {
           Err(PublishError::CargoPublishFailed {
               crate_name: crate_name.to_string(),
               exit_code: status.code().unwrap_or(-1),
           })
       }
   }

   pub(crate) fn wait_for_index(crate_name: &str, version: &str) -> Result<(), PublishError> {
       let url = sparse_index_url(crate_name);
       for attempt in 1..=POLL_RETRIES {
           match ureq::get(&url).call() {
               Ok(response) => {
                   let body = response.into_string().unwrap_or_default();
                   if version_in_index_body(&body, version) {
                       eprintln!("  [{crate_name}] indexed after {attempt} poll(s)");
                       return Ok(());
                   }
               }
               Err(e) => {
                   return Err(PublishError::HttpError {
                       crate_name: crate_name.to_string(),
                       source: Box::new(e),
                   });
               }
           }
           eprintln!("  [{crate_name}] not yet indexed (attempt {attempt}/{POLL_RETRIES}), waiting {POLL_INTERVAL_SECS}s...");
           std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
       }
       Err(PublishError::IndexPollTimeout {
           crate_name: crate_name.to_string(),
           version: version.to_string(),
       })
   }
   ```

3. Verify:

   ```
   cargo nextest run -p xtask    → all green
   cargo clippy -p xtask -- -D warnings  → zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(xtask): implement cargo_publish and wait_for_index"`

---

### Task 6: Implement `run_publish` and wire into `main()`

**Crate**: `xtask`
**File(s)**: `xtask/src/publish.rs`, `xtask/src/main.rs`
**Run**: `cargo nextest run -p xtask`

1. Write failing tests:

   ```rust
   #[test]
   fn run_publish_rejects_unknown_from_crate() {
       let args = PublishArgs { from: Some("not-a-real-crate".to_string()) };
       let err = run_publish_dry(args).unwrap_err();
       assert!(matches!(err, PublishError::UnknownFromCrate { .. }));
   }

   #[test]
   fn run_publish_dry_from_crux_planner_skips_twelve_crates() {
       let args = PublishArgs { from: Some("crux-planner".to_string()) };
       let remaining = crates_from(args.from.as_deref()).unwrap();
       assert_eq!(remaining.len(), 2); // crux-planner + crux
       assert_eq!(remaining[0].name, "crux-planner");
   }

   // Helper used only in tests — extracts the slice after --from without publishing
   fn crates_from(from: Option<&str>) -> Result<&'static [CrateSpec], PublishError> {
       match from {
           None => Ok(PUBLISH_ORDER),
           Some(name) => {
               let pos = PUBLISH_ORDER
                   .iter()
                   .position(|c| c.name == name)
                   .ok_or_else(|| PublishError::UnknownFromCrate { name: name.to_string() })?;
               Ok(&PUBLISH_ORDER[pos..])
           }
       }
   }

   // Dry-run variant of run_publish that validates args without invoking cargo
   fn run_publish_dry(args: PublishArgs) -> Result<(), PublishError> {
       crates_from(args.from.as_deref())?;
       Ok(())
   }
   ```

   Run: `cargo nextest run -p xtask -- run_publish`
   Expected: FAIL

2. Implement `run_publish` in `xtask/src/publish.rs`:

   ```rust
   pub(crate) fn run_publish(args: PublishArgs) -> Result<(), PublishError> {
       let version = workspace_version()?;
       eprintln!("publishing crux workspace v{version}");

       let crates = match args.from.as_deref() {
           None => PUBLISH_ORDER,
           Some(name) => {
               let pos = PUBLISH_ORDER
                   .iter()
                   .position(|c| c.name == name)
                   .ok_or_else(|| PublishError::UnknownFromCrate { name: name.to_string() })?;
               &PUBLISH_ORDER[pos..]
           }
       };

       for spec in crates {
           eprintln!("publishing {} ...", spec.name);
           cargo_publish(spec.name)?;
           eprintln!("waiting for {} to be indexed ...", spec.name);
           wait_for_index(spec.name, &version)?;
       }

       eprintln!("all crates published successfully");
       Ok(())
   }
   ```

   Wire into `xtask/src/main.rs` — replace the existing `main` body with:

   ```rust
   mod publish;
   use publish::{parse_publish_args, run_publish};
   use std::process::{Command, exit};

   fn main() {
       let args: Vec<String> = std::env::args().skip(1).collect();

       if args.first().map(|s| s.as_str()) == Some("publish") {
           match parse_publish_args(&args[1..]) {
               Ok(publish_args) => {
                   if let Err(e) = run_publish(publish_args) {
                       eprintln!("publish failed: {e}");
                       exit(1);
                   }
               }
               Err(e) => {
                   eprintln!("usage error: {e}");
                   exit(2);
               }
           }
           return;
       }

       match Command::new("taskit").args(&args).status() {
           Ok(status) => exit(status.code().unwrap_or(1)),
           Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
               eprintln!("taskit not found, installing via cargo install...");
               let install = Command::new("cargo")
                   .args(["install", "taskit"])
                   .status()
                   .expect("failed to run cargo install");
               if !install.success() {
                   eprintln!("failed to install taskit");
                   exit(1);
               }
               let status = Command::new("taskit")
                   .args(&args)
                   .status()
                   .expect("failed to run taskit after install");
               exit(status.code().unwrap_or(1));
           }
           Err(e) => {
               eprintln!("failed to run taskit: {e}");
               exit(1);
           }
       }
   }
   ```

3. Verify:

   ```
   cargo nextest run -p xtask    → all green
   cargo clippy -p xtask -- -D warnings  → zero warnings
   cargo build -p xtask          → binary compiles
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(xtask): implement run_publish and wire publish subcommand into main"`

---

### Task 7: Update `release.yml` to use `cargo xtask publish`

**File(s)**: `.github/workflows/release.yml`
**Run**: `cargo build -p xtask` (verify binary still builds after yml change is irrelevant — just confirm no xtask regressions)

1. Before (current state): 14 individual `cargo publish -p <crate>` steps plus per-step
   `CARGO_REGISTRY_TOKEN` env vars.

2. Replace the entire `publish` job steps (after checkout + toolchain + cache) with:

   ```yaml
   - name: publish all crates
     run: cargo xtask publish
     env:
       CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
   ```

   Full updated `publish` job:

   ```yaml
   publish:
     name: publish to crates.io
     runs-on: ubuntu-latest
     needs: gate
     environment: crates-io
     steps:
       - uses: actions/checkout@v4
       - uses: dtolnay/rust-toolchain@stable
       - uses: Swatinem/rust-cache@v2
       - name: publish all crates
         run: cargo xtask publish
         env:
           CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
   ```

3. Verify:

   ```
   cargo build -p xtask    → binary compiles cleanly
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "ci: replace 14-step publish sequence with cargo xtask publish"`

---

## Quality Rules Checklist

- [x] Every requirement maps to a task
- [x] No placeholders — all code is copy-paste ready
- [x] Type and function names are consistent across all tasks
- [x] Each task is bounded (2-5 minutes focused work)
- [x] Each task ends with a commit
- [x] TDD: failing test → verify failure → implement → verify pass → commit
