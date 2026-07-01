---
crate: crux-plugin
role: plugin-host
howto:
  - task: "Add plugin support"
    locations:
      - "src/protocol.rs — add new JSON-RPC methods"
      - "src/manifest.rs — extend manifest format"
      - "src/bridge.rs — update handler bridging"
  - task: "Write a plugin"
    steps:
      - "Create binary reading JSON-RPC from stdin, writing to stdout"
      - "Add plugin.json manifest declaring supported step types"
      - "Place in directory scanned by discovery"
test_fixture: "src/bin/echo-plugin.rs"
---

# Agents: crux-plugin

## For AI Agents Working With This Crate

Subprocess plugin host. Extends pipelines with external binaries.

### Writing a Plugin

1. Create a binary that reads JSON-RPC from stdin, writes to stdout
2. Add a `plugin.json` manifest declaring supported step types
3. Place in a directory scanned by `discovery`

### Testing

Use the `echo-plugin` binary (`src/bin/echo-plugin.rs`) as a reference.
Integration tests spawn it as a child process.
