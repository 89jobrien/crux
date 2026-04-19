#!/usr/bin/env fish
# crux developer setup — fish version
#
# PARITY NOTICE: this script must stay in sync with setup.sh and setup.nu.
# All three must perform the same steps in the same order. When editing one,
# update the other two.
#
# Usage: fish scripts/setup.fish

set -g errors 0

function header
    printf "\n\033[1;34m==> %s\033[0m\n" $argv[1]
end

function ok
    printf "  \033[32m[ok]\033[0m %s\n" $argv[1]
end

function warn
    printf "  \033[33m[warn]\033[0m %s\n" $argv[1]
end

function fail
    printf "  \033[31m[fail]\033[0m %s\n" $argv[1]
    set -g errors (math $errors + 1)
end

# --- 1. Rust toolchain ---
header "Rust toolchain"
if command -q rustc
    set rust_ver (rustc --version | string match -r '\d+\.\d+\.\d+')
    set parts (string split . $rust_ver)
    set major $parts[1]
    set minor $parts[2]
    if test $major -ge 1; and test $minor -ge 85
        ok "rustc $rust_ver (>= 1.85)"
    else
        fail "rustc $rust_ver is below MSRV 1.85 — run: rustup update stable"
    end
else
    fail "rustc not found — install from https://rustup.rs"
end

# --- 2. Required tools ---
header "Required tools"
for tool in just cargo-nextest mise nu
    if test $tool = cargo-nextest
        if cargo nextest --version >/dev/null 2>&1
            ok $tool
        else
            warn "$tool not found — install: cargo install cargo-nextest"
        end
        continue
    end
    if command -q $tool
        ok $tool
    else
        warn "$tool not found"
    end
end

# --- 3. mise install (baml-cli) ---
header "mise install"
if command -q mise
    if mise install 2>&1
        ok "mise install completed"
    else
        warn "mise install had issues — check output above"
    end
else
    warn "mise not found — skipping baml-cli install"
end

# --- 4. Generate BAML client ---
header "BAML client generation"
set baml_dir crates/crux-agentic
if test -d $baml_dir/baml_src
    if command -q mise
        if bash -c "cd $baml_dir && mise exec -- baml-cli generate" 2>&1
            ok "baml_client/ generated in $baml_dir"
        else
            fail "baml-cli generate failed"
        end
    else
        warn "mise not available — cannot generate baml_client/"
        warn "run manually: cd $baml_dir && mise exec -- baml-cli generate"
    end
else
    warn "baml_src/ not found — skipping"
end

# --- 5. Git hooks ---
header "Git hooks"
if test -d .githooks
    git config core.hooksPath .githooks
    ok "core.hooksPath set to .githooks/"
else
    warn ".githooks/ directory not found — skipping"
end

# --- 6. Environment file ---
header "Environment"
if test -f .env
    ok ".env already exists"
else if test -f .env.example
    cp .env.example .env
    ok "copied .env.example -> .env (fill in your API keys)"
else
    warn ".env.example not found — skipping"
end

# --- 7. LLM API keys ---
header "LLM API keys"
set has_openai false
set has_anthropic false
if set -q OPENAI_API_KEY; and test -n "$OPENAI_API_KEY"
    set has_openai true
end
if set -q ANTHROPIC_API_KEY; and test -n "$ANTHROPIC_API_KEY"
    set has_anthropic true
end
if test $has_openai = true; and test $has_anthropic = true
    ok "OPENAI_API_KEY and ANTHROPIC_API_KEY are set"
else if test $has_openai = true
    ok "OPENAI_API_KEY is set"
    warn "ANTHROPIC_API_KEY is not set"
else if test $has_anthropic = true
    ok "ANTHROPIC_API_KEY is set"
    warn "OPENAI_API_KEY is not set"
else
    warn "neither OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
    warn "LLM features (llm::*, plan) will not work without at least one"
    warn "configure in .env or inject via dotenvx/direnv"
end

# --- 8. Build ---
header "Build"
if cargo build --all-targets 2>&1
    ok "cargo build --all-targets succeeded"
else
    fail "cargo build failed"
end

# --- Summary ---
echo ""
if test $errors -eq 0
    printf "\033[1;32mSetup complete — no errors.\033[0m\n"
else
    printf "\033[1;31mSetup finished with %d error(s).\033[0m\n" $errors
end
