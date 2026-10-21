# crux-plugin

Subprocess plugin host for extending Crux pipelines with external executables. Plugins declare
handlers and exchange newline-delimited JSON messages over stdin/stdout.

## Architecture role

`PluginDiscovery` is the discovery port and `TomlFileDiscovery` is its filesystem adapter.
`PluginHost` owns child processes and protocol I/O. `register_plugins` loads entries and bridges
each declared handler into `crux_script::HandlerRegistry`.

## Manifest and usage

```toml
[[plugin]]
name = "echo"
path = "/path/to/echo-plugin"
env = { EXAMPLE_MODE = "test" }
```

```rust
use crux_plugin::bridge::register_plugins;
use crux_plugin::manifest::load_manifest;
use crux_script::HandlerRegistry;

let manifest = load_manifest("plugins.toml")?;
let mut registry = HandlerRegistry::new();
register_plugins(&mut registry, &manifest.plugin).await?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Missing manifests load as an empty configuration. The CLI default location is
`$HOME/.crux/plugins.toml`, and `crux run --plugins` can override it.

## Protocol and key API

- `Request`: `Declare`, `Invoke { handler, input }`, and `Shutdown`.
- `Response`: declarations, successful output, string errors, and shutdown acknowledgement.
- `HandlerDecl`: namespaced handler name and planner-facing description.
- `PluginHost`: load, inspect declarations, invoke, and shut down plugins.
- `PluginManifest`, `PluginEntry`, `load_manifest`, `PluginDiscovery`, `TomlFileDiscovery`.

The test `echo-plugin` is a source-grounded minimal implementation: read one JSON request per line,
write one response per line, and flush stdout after each response.

## Features and implementation status

There are no Cargo features or generated files. The protocol is JSON-RPC-like but currently has no
version negotiation, request IDs, concurrent correlated calls, streaming, cancellation, deadlines,
or structured errors. Bridged invocations share one host mutex and are serialized. Manifest `env`
values are literal process environment values, so secrets should be injected rather than committed.

## Development and testing

```console
cargo nextest run -p crux-plugin
cargo clippy -p crux-plugin --all-targets -- -D warnings
```

Integration tests exercise manifest parsing, protocol serde, process lifecycle, invocation, bridge
registration, missing files, and the echo fixture binary.
