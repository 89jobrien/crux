---
crate: crux-plugin
type: host
description: "Subprocess plugin host for crux pipelines"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-script
  - serde_json
  - tokio
protocol: "JSON-RPC over stdin/stdout"
modules:
  - name: host
    purpose: "Plugin lifecycle management"
  - name: bridge
    purpose: "Handler bridge into HandlerRegistry"
  - name: discovery
    purpose: "Plugin manifest scanning"
  - name: manifest
    purpose: "Manifest format and parsing"
  - name: protocol
    purpose: "JSON-RPC message types"
binaries:
  - name: echo-plugin
    purpose: "Test plugin for integration tests"
---

# crux-plugin

Subprocess plugin host for crux pipelines. Plugins are external binaries
speaking a JSON-RPC protocol over stdin/stdout. The host discovers them
from a manifest, launches them as persistent child processes, and bridges
their handlers into the crux `HandlerRegistry`.

## Plugin Protocol

Plugins communicate via JSON-RPC over stdin/stdout. Each plugin declares
its supported step types in a manifest file. The host routes matching
steps to the plugin process and collects results.
