---
crate: crux-plugin
pattern: subprocess-host
protocol: "JSON-RPC over stdio"
lifecycle:
  - step: discover
    function: "discovery::scan_manifests()"
  - step: launch
    function: "host::launch()"
  - step: bridge
    function: "bridge::register()"
  - step: execute
    description: "Pipeline dispatches steps to plugin"
  - step: shutdown
    function: "host::shutdown()"
modules:
  - name: manifest
    role: "Plugin manifest format (plugin.json)"
  - name: discovery
    role: "Scan directories for manifests"
  - name: host
    role: "Process lifecycle (spawn, health, shutdown)"
  - name: protocol
    role: "JSON-RPC message types"
  - name: bridge
    role: "Adapt plugin calls to HandlerRegistry"
---

# Architecture: crux-plugin

Subprocess plugin host using JSON-RPC over stdio.

## Lifecycle

```
1. discovery::scan_manifests()  — find plugin manifests
2. host::launch()               — spawn plugin process
3. bridge::register()           — bridge handlers into registry
4. (pipeline runs, steps dispatched to plugin)
5. host::shutdown()             — terminate plugin process
```

## Protocol

```
Host -> Plugin: { "method": "execute", "params": { step_def } }
Plugin -> Host: { "result": { output } }
```

Messages are newline-delimited JSON over stdin/stdout. Stderr is
forwarded to the host's logging.
