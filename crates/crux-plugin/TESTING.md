---
crate: crux-plugin
test_strategy: mixed
inline_test_modules: 1
dedicated_test_files: 5
test_areas:
  - module: manifest
    coverage: "Plugin manifest parsing"
  - module: protocol
    coverage: "JSON-RPC message serialization"
  - module: discovery
    coverage: "Manifest scanning"
  - module: host
    coverage: "Process spawn and shutdown"
  - module: bridge
    coverage: "Handler registration from plugin"
test_fixtures:
  - name: echo-plugin
    path: src/bin/echo-plugin.rs
    purpose: "Test plugin for integration tests"
commands:
  default: "cargo nextest run -p crux-plugin"
---

# Testing: crux-plugin

## Test Strategy

1 inline test module + 5 dedicated test files. Tests cover manifest
parsing, protocol messages, and plugin lifecycle.

## Running

```bash
cargo nextest run -p crux-plugin
```

## Echo Plugin

`src/bin/echo-plugin.rs` is a test plugin that echoes input back. Used
in integration tests to verify the full plugin lifecycle.
