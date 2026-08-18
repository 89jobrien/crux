# crux toolkit — auto-loaded by toolkit-hook when you cd into this repo

export def "check"  [] { cargo check --workspace }
export def "lint"   [] { cargo clippy --workspace -- -D warnings }
export def "fmt"    [] { cargo fmt --all }
export def "test"   [] { cargo nextest run --workspace }
export def "build"  [] { cargo build --workspace }
export def "clean"  [] { cargo clean }

export def "help" [] {
    scope commands
    | where name =~ "^tk "
    | select name
    | sort-by name
}
