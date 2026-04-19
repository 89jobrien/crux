#!/usr/bin/env nu
# crux developer setup — nushell version
#
# PARITY NOTICE: this script must stay in sync with setup.sh and setup.fish.
# All three must perform the same steps in the same order. When editing one,
# update the other two.
#
# Usage: nu scripts/setup.nu

mut errors = 0

def header [msg: string] {
    print $"\n(ansi blue_bold)==> ($msg)(ansi reset)"
}

def ok [msg: string] {
    print $"  (ansi green)[ok](ansi reset) ($msg)"
}

def warn [msg: string] {
    print $"  (ansi yellow)[warn](ansi reset) ($msg)"
}

def fail [msg: string] {
    print $"  (ansi red)[fail](ansi reset) ($msg)"
}

# --- 1. Rust toolchain ---
header "Rust toolchain"
let rustc_found = (which rustc | length) > 0
if $rustc_found {
    let rust_ver = (rustc --version | parse --regex '(\d+\.\d+\.\d+)' | get capture0 | first)
    let parts = ($rust_ver | split row ".")
    let major = ($parts | get 0 | into int)
    let minor = ($parts | get 1 | into int)
    if $major >= 1 and $minor >= 85 {
        ok $"rustc ($rust_ver) \(>= 1.85\)"
    } else {
        fail $"rustc ($rust_ver) is below MSRV 1.85 — run: rustup update stable"
        $errors = $errors + 1
    }
} else {
    fail "rustc not found — install from https://rustup.rs"
    $errors = $errors + 1
}

# --- 2. Required tools ---
header "Required tools"
for tool in [just mise] {
    if (which $tool | length) > 0 {
        ok $tool
    } else {
        warn $"($tool) not found"
    }
}

# cargo-nextest check
let nextest_result = (do { cargo nextest --version } | complete)
if $nextest_result.exit_code == 0 {
    ok "cargo-nextest"
} else {
    warn "cargo-nextest not found — install: cargo install cargo-nextest"
}

# nu is obviously present if we're running this
ok "nu"

# --- 3. mise install (baml-cli) ---
header "mise install"
if (which mise | length) > 0 {
    let result = (do { mise install } | complete)
    if $result.exit_code == 0 {
        ok "mise install completed"
    } else {
        warn "mise install had issues — check output above"
        print $result.stderr
    }
} else {
    warn "mise not found — skipping baml-cli install"
}

# --- 4. Generate BAML client ---
header "BAML client generation"
let baml_dir = "crates/crux-agentic"
if ($"($baml_dir)/baml_src" | path exists) {
    if (which mise | length) > 0 {
        let result = (do { cd $baml_dir; mise exec -- baml-cli generate } | complete)
        if $result.exit_code == 0 {
            ok $"baml_client/ generated in ($baml_dir)"
        } else {
            fail "baml-cli generate failed"
            print $result.stderr
            $errors = $errors + 1
        }
    } else {
        warn "mise not available — cannot generate baml_client/"
        warn $"run manually: cd ($baml_dir) && mise exec -- baml-cli generate"
    }
} else {
    warn "baml_src/ not found — skipping"
}

# --- 5. Git hooks ---
header "Git hooks"
if (".githooks" | path exists) {
    git config core.hooksPath .githooks
    ok "core.hooksPath set to .githooks/"
} else {
    warn ".githooks/ directory not found — skipping"
}

# --- 6. Environment file ---
header "Environment"
if (".env" | path exists) {
    ok ".env already exists"
} else if (".env.example" | path exists) {
    cp .env.example .env
    ok "copied .env.example -> .env (fill in your API keys)"
} else {
    warn ".env.example not found — skipping"
}

# --- 7. Build ---
header "Build"
let build_result = (do { cargo build --all-targets } | complete)
if $build_result.exit_code == 0 {
    ok "cargo build --all-targets succeeded"
} else {
    fail "cargo build failed"
    print $build_result.stderr
    $errors = $errors + 1
}

# --- Summary ---
print ""
if $errors == 0 {
    print $"(ansi green_bold)Setup complete — no errors.(ansi reset)"
} else {
    print $"(ansi red_bold)Setup finished with ($errors) error\(s\).(ansi reset)"
}
