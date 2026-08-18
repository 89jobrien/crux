# Design: `cargo xtask publish`

## Goal

Add a `publish` subcommand to the xtask shim that publishes all 14 crux crates in
dependency order, polling the crates.io sparse index after each publish until the new
version is indexed, with `--from <crate>` for mid-run resume.

## Approved Approach

Poll-based publish sequencer in xtask (Rust): intercept `publish` before delegating to
taskit, invoke `cargo publish`, then poll the crates.io sparse index with `ureq` until
the version appears.

## Crate Ownership

- **Owner crate**: `xtask` — it is the existing task runner binary; publish orchestration
  is a release task, not domain logic
- **Affected crates**: none (xtask is `publish = false` and imported by nothing)

## New Dependencies (xtask/Cargo.toml)

Both are already in the workspace dependency tree as transitives:

```toml
ureq = "2"
serde_json = "1"
```

No workspace-level change needed — xtask is not a workspace-dep consumer.

## Public API

xtask is a binary; all items are crate-private. Signatures only:

### Types

```rust
struct PublishArgs {
    from: Option<String>,
}

struct CrateSpec {
    name: &'static str,
}

enum PublishError {
    CargoPublishFailed { crate_name: String, exit_code: i32 },
    IndexPollTimeout { crate_name: String, version: String },
    HttpError { crate_name: String, source: Box<ureq::Error> },
    VersionNotInWorkspace,
    UnknownFromCrate { name: String },
}

impl std::fmt::Display for PublishError { ... }
impl std::error::Error for PublishError { ... }
```

### Constants

```rust
const PUBLISH_ORDER: &[CrateSpec] = &[
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

const POLL_RETRIES: u32 = 30;
const POLL_INTERVAL_SECS: u64 = 10;
```

### Functions

```rust
// Entry point called from main() when first arg is "publish"
fn run_publish(args: PublishArgs) -> Result<(), PublishError>;

// Read version from workspace Cargo.toml [workspace.package] version field
fn workspace_version() -> Result<String, PublishError>;

// Invoke `cargo publish -p <name>` as a subprocess; inherit stdio
fn cargo_publish(crate_name: &str) -> Result<(), PublishError>;

// Poll sparse index until version appears or timeout
fn wait_for_index(crate_name: &str, version: &str) -> Result<(), PublishError>;

// Build sparse index URL for a crate (name must be >= 4 chars)
fn sparse_index_url(crate_name: &str) -> String;

// Check whether a specific version line exists in the index response body
fn version_in_index_body(body: &str, version: &str) -> bool;

// Parse --from flag from raw args; returns Err if flag present but value missing
fn parse_publish_args(args: &[String]) -> Result<PublishArgs, String>;
```

## Data Flow

1. **main()** receives `["publish", ...]`; calls `parse_publish_args`, then `run_publish`
2. **run_publish** calls `workspace_version()` to read the version from `Cargo.toml`
3. **run_publish** iterates `PUBLISH_ORDER`; if `--from` is set, skips until the named
   crate is reached
4. For each crate: calls `cargo_publish(name)` → on success calls `wait_for_index(name,
   version)` → on success advances to next crate
5. **wait_for_index** builds the sparse index URL, GETs it with `ureq`, searches the
   newline-delimited JSON body for `"vers":"<version>"`, sleeps and retries up to 30×
6. On any error, prints a human-readable message and exits non-zero

## Hexagonal Boundaries

This is a CLI task runner, not a domain component. No port/adapter split is needed —
the only external dependency (crates.io HTTP) is used directly in `wait_for_index`.
If the polling strategy ever needs to be swapped (e.g., using the crates.io REST API
instead of the sparse index), `wait_for_index` is the sole site to change.

## Integration with main()

```rust
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(|s| s.as_str()) == Some("publish") {
        match parse_publish_args(&args[1..]) {
            Ok(publish_args) => {
                if let Err(e) = run_publish(publish_args) {
                    eprintln!("publish failed: {e}");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("usage error: {e}");
                std::process::exit(2);
            }
        }
        return;
    }

    // existing taskit delegation ...
}
```

## release.yml change

Replace the 14-step publish sequence with:

```yaml
- name: publish all crates
  run: cargo xtask publish
  env:
    CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

Resume example (if re-running after partial failure at crux-stdlib):

```yaml
run: cargo xtask publish --from crux-stdlib
```

## Out of Scope

- Parallel publishing within a layer
- Crate names shorter than 4 characters (none exist in this workspace)
- Authentication — caller sets `CARGO_REGISTRY_TOKEN`
- `--dry-run` flag
- Yanking or rollback on partial failure

## Risk

- [ ] Breaking API changes: no (xtask is `publish = false`, no consumers)
- [ ] New external dependencies: yes — `ureq = "2"` and `serde_json = "1"` added to
      `xtask/Cargo.toml` only
- [ ] Feature flag required: no
