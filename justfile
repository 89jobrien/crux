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

# Run full CI suite locally (fmt + clippy + test)
ci: fmt lint test

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
