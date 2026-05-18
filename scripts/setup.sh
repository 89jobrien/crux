#!/usr/bin/env bash
# cruxx developer setup — bash version
#
# PARITY NOTICE: this script must stay in sync with setup.nu and setup.fish.
# All three must perform the same steps in the same order. When editing one,
# update the other two.
#
# Usage: ./scripts/setup.sh

set -euo pipefail

errors=0

header() { printf "\n\033[1;34m==> %s\033[0m\n" "$1"; }
ok()     { printf "  \033[32m[ok]\033[0m %s\n" "$1"; }
warn()   { printf "  \033[33m[warn]\033[0m %s\n" "$1"; }
fail()   { printf "  \033[31m[fail]\033[0m %s\n" "$1"; errors=$((errors + 1)); }

# --- 1. Rust toolchain ---
header "Rust toolchain"
if command -v rustc >/dev/null 2>&1; then
    rust_ver=$(rustc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
    major=$(echo "$rust_ver" | cut -d. -f1)
    minor=$(echo "$rust_ver" | cut -d. -f2)
    if [ "$major" -ge 1 ] && [ "$minor" -ge 88 ]; then
        ok "rustc $rust_ver (>= 1.88)"
    else
        fail "rustc $rust_ver is below MSRV 1.88 — run: rustup update stable"
    fi
else
    fail "rustc not found — install from https://rustup.rs"
fi

# --- 2. Required tools ---
header "Required tools"
for tool in just cargo-nextest mise nu; do
    cmd="$tool"
    if [ "$tool" = "cargo-nextest" ]; then
        cmd="cargo-nextest"
        if cargo nextest --version >/dev/null 2>&1; then
            ok "$tool"
        else
            warn "$tool not found — install: cargo install cargo-nextest"
        fi
        continue
    fi
    if command -v "$cmd" >/dev/null 2>&1; then
        ok "$tool"
    else
        warn "$tool not found"
    fi
done

# --- 3. mise install (baml-cli) ---
header "mise install"
if command -v mise >/dev/null 2>&1; then
    if mise install 2>&1; then
        ok "mise install completed"
    else
        warn "mise install had issues — check output above"
    fi
else
    warn "mise not found — skipping baml-cli install"
fi

# --- 4. Generate BAML client ---
header "BAML client generation"
baml_dir="crates/cruxx-agentic"
if [ -d "$baml_dir/baml_src" ]; then
    if command -v mise >/dev/null 2>&1; then
        if (cd "$baml_dir" && mise exec -- baml-cli generate) 2>&1; then
            ok "baml_client/ generated in $baml_dir"
        else
            fail "baml-cli generate failed"
        fi
    else
        warn "mise not available — cannot generate baml_client/"
        warn "run manually: cd $baml_dir && mise exec -- baml-cli generate"
    fi
else
    warn "baml_src/ not found — skipping"
fi

# --- 5. Git hooks ---
header "Git hooks"
if [ -d ".githooks" ]; then
    git config core.hooksPath .githooks
    ok "core.hooksPath set to .githooks/"
else
    warn ".githooks/ directory not found — skipping"
fi

# --- 6. Environment file ---
header "Environment"
if [ -f ".env" ]; then
    ok ".env already exists"
elif [ -f ".env.example" ]; then
    cp .env.example .env
    ok "copied .env.example -> .env (fill in your API keys)"
else
    warn ".env.example not found — skipping"
fi

# --- 7. LLM API keys ---
header "LLM API keys"
has_openai=false
has_anthropic=false
if [ -n "${OPENAI_API_KEY:-}" ]; then has_openai=true; fi
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then has_anthropic=true; fi
if [ "$has_openai" = true ] && [ "$has_anthropic" = true ]; then
    ok "OPENAI_API_KEY and ANTHROPIC_API_KEY are set"
elif [ "$has_openai" = true ]; then
    ok "OPENAI_API_KEY is set"
    warn "ANTHROPIC_API_KEY is not set"
elif [ "$has_anthropic" = true ]; then
    ok "ANTHROPIC_API_KEY is set"
    warn "OPENAI_API_KEY is not set"
else
    warn "neither OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
    warn "LLM features (llm::*, plan) will not work without at least one"
    warn "configure in .env or inject via dotenvx/direnv"
fi

# --- 8. Build ---
header "Build"
if cargo build --all-targets 2>&1; then
    ok "cargo build --all-targets succeeded"
else
    fail "cargo build failed"
fi

# --- Summary ---
echo ""
if [ "$errors" -eq 0 ]; then
    printf "\033[1;32mSetup complete — no errors.\033[0m\n"
else
    printf "\033[1;31mSetup finished with %d error(s).\033[0m\n" "$errors"
fi
