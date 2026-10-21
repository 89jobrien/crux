# Editor integration

`crux init` already writes the wiring below for you. To add it to an existing
project:

```bash
crux schema --editor yaml-language-server
```

That prints the `yaml.schemas` block along with the command that exports the
schema file it points at. Alternatively, generate both by hand:

```bash
mkdir -p .vscode
crux schema --output .vscode/crux-schema.json
```

For VS Code with the YAML extension, add this to `.vscode/settings.json`:

```json
{
  "files.associations": { "*.crux": "yaml" },
  "yaml.schemas": { "./.vscode/crux-schema.json": ["*.crux", "Cruxfile"] }
}
```

The `Cruxfile` pattern matters: Cruxfiles have no extension, so they need an
explicit match to get completions.

Other editors can associate `.vscode/crux-schema.json` with `*.crux` and
`Cruxfile` through their YAML language-server configuration. Diagnostics and
completions then use the same schema as `crux check`.

## Verifying your installation

The schema and the CLI's default output both drift when an old `crux` is on
`PATH`. `crux version` reports the build provenance of the running binary:

```bash
crux version          # version, commit, build time, profile, binary path
crux version --check  # compare against HEAD of the surrounding crux checkout
```

`--check` exits `0` when current, `1` when the binary predates the checkout's
HEAD, and `2` when it cannot tell. See the "Pipeline Binary" section of
`AGENTS.md` for the reinstall command.
