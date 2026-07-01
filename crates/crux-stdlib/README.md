---
crate: crux-stdlib
type: handlers
description: "Standard library handlers for crux-script pipelines"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-script
handler_domains:
  - name: fs
    handlers: "File read/write/copy/move/delete"
  - name: git
    handlers: "Git operations (status, diff, commit, branch)"
  - name: json
    handlers: "JSON transforms (jq-style), merge, extract"
  - name: text
    handlers: "Text parsing, regex, splitting"
  - name: shell
    handlers: "Shell command execution"
  - name: ctrl
    handlers: "Control flow: conditional, loop, parallel"
---

# crux-stdlib

Standard library handlers for crux-script pipelines. Deterministic,
non-agentic utilities: filesystem, git, JSON transforms, text parsing,
shell execution, and control flow primitives.

## Usage

```rust
use crux_stdlib::register_all;
use crux_script::HandlerRegistry;

let mut registry = HandlerRegistry::new();
register_all(&mut registry);
```
