default:
    @just --list

# Run cargo fmt check
fmt:
    cargo fmt --all -- --check

# Run clippy with deny warnings
lint:
    cargo clippy --all-targets -- -D warnings

# Run all tests via nextest
test:
    cargo nextest run

# Run full CI suite locally (fmt + clippy + test + baml check)
ci: fmt lint test check-baml

# Build all targets
build:
    cargo build --all-targets

# Install git hooks
hooks:
    git config core.hooksPath .githooks
    @echo "Git hooks installed from .githooks/"

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
        error make { msg: $"BAML version mismatch: generators.baml=($gen_ver) Cargo.toml=($cargo_ver)" }
    }
    let lib = $"($env.HOME)/Library/Caches/baml/libs/($gen_ver)/libbaml_cffi-aarch64-apple-darwin.dylib"
    if not ($lib | path exists) {
        print $"BAML native lib not cached for ($gen_ver) — downloading..."
        mkdir ($lib | path dirname)
        let url = $"https://github.com/boundaryml/baml/releases/download/($gen_ver)/libbaml_cffi-aarch64-apple-darwin.dylib"
        http get $url | save --force $lib
    }
    print $"BAML versions match: ($gen_ver)"
