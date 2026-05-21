default:
    @just --list

# Run cargo fmt check (DO NOT CHANGE IF YOU DO NOT HAVE A FINGERPRINT)
fmt:
    cargo fmt --all -- --check

# Run cargo check with warnings-as-errors (DO NOT CHANGE IF YOU DO NOT HAVE A FINGERPRINT)
check:
    RUSTFLAGS="-D warnings" cargo check --workspace --all-targets

# Run clippy with deny warnings (DO NOT CHANGE IF YOU DO NOT HAVE A FINGERPRINT)
lint:
    cargo clippy --all-targets -- -D warnings

# Run all tests via nextest (DO NOT CHANGE IF YOU DO NOT HAVE A FINGERPRINT)
test:
    cargo nextest run

# Run full CI suite locally (mirrors GH Actions - DO NOT CHANGE IF YOU DO NOT HAVE A FINGERPRINT)
ci: check build-locked fmt lint test deny lint-crux

# Build all targets
build:
    cargo build --all-targets

# Build all targets with --locked and warnings-as-errors (CI parity)
build-locked:
    RUSTFLAGS="-D warnings" cargo build --locked --all-targets

# Build with all features (baml, plugins)
build-full:
    cargo build --all-targets -p crux-agentic --features baml

# Build dev binary with all features and install to cargo bin
build-dev:
    cargo build -p crux-agentic --features baml
    cargo install --path crates/crux-agentic --features baml

# Run developer setup (auto-detects shell)
setup:
    #!/usr/bin/env bash
    if command -v nu >/dev/null 2>&1; then
        nu scripts/setup.nu
    elif command -v fish >/dev/null 2>&1; then
        fish scripts/setup.fish
    else
        bash scripts/setup.sh
    fi

# Install git hooks
hooks:
    git config core.hooksPath .githooks
    @echo "Git hooks installed from .githooks/"

# Run cargo-deny (license + advisory audit)
deny:
    cargo deny check 2>/dev/null || echo "cargo-deny not installed — skipping (install: cargo install cargo-deny)"

# Format code in place
fix:
    cargo fmt --all

# Check BAML generator version matches baml crate version in Cargo.toml
check-baml:
    #!/usr/bin/env nu
    let gen_ver = (open crates/crux-agentic/baml_src/generators.baml
        | lines
        | where { |l| $l =~ 'version' }
        | first
        | parse --regex '"([0-9]+\.[0-9]+\.[0-9]+)"'
        | get capture0
        | first)
    let cargo_ver = (open --raw crates/crux-agentic/Cargo.toml
        | lines
        | where { |l| $l =~ 'version = "[0-9]' and ($l =~ '^baml') }
        | first
        | parse --regex 'version = "([0-9]+\.[0-9]+\.[0-9]+)"'
        | get capture0
        | first)
    if $gen_ver != $cargo_ver {
        print $"(ansi red_bold)BAML version mismatch(ansi reset)"
        print $"  generators.baml  → ($gen_ver)"
        print $"  Cargo.toml       → ($cargo_ver)"
        print ""
        print $"(ansi yellow)Fix: update the baml dep in crates/crux-agentic/Cargo.toml to match:(ansi reset)"
        print $"  baml = \{ version = \"($gen_ver)\", optional = true \}"
        error make { msg: "baml version mismatch" }
    }
    let lib = $"($env.HOME)/Library/Caches/baml/libs/($gen_ver)/libbaml_cffi-aarch64-apple-darwin.dylib"
    if not ($lib | path exists) {
        print $"BAML native lib not cached for ($gen_ver) — downloading..."
        mkdir ($lib | path dirname)
        let url = $"https://github.com/boundaryml/baml/releases/download/($gen_ver)/libbaml_cffi-aarch64-apple-darwin.dylib"
        http get $url | save --force $lib
    }
    print $"BAML versions match: ($gen_ver)"

# Lint all .crux pipeline files (parse + handler/arg validation)
lint-crux:
    #!/usr/bin/env bash
    files=$(find examples -name '*.crux' | sort)
    if [ -z "$files" ]; then
        echo "No .crux files found"
        exit 0
    fi
    cargo run --quiet -p crux-agentic --bin crux -- check $files

# Run hook bats tests
test-hooks:
    bats scripts/tests/hooks.bats

# Run hook fuzz suite (default 500 iterations)
fuzz-hooks iterations="500":
    bash scripts/tests/fuzz.sh --iterations {{iterations}}
