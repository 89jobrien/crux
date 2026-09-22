# Editor integration

Generate the pipeline schema inside your project:

```bash
mkdir -p .crux
crux schema --output .crux/pipeline.schema.json
```

For VS Code with the YAML extension, add this to `.vscode/settings.json`:

```json
{
  "files.associations": { "*.crux": "yaml" },
  "yaml.schemas": { ".crux/pipeline.schema.json": "*.crux" }
}
```

Other editors can associate `.crux/pipeline.schema.json` with `*.crux` through
their YAML language-server configuration. Diagnostics and completions then use
the same schema as `crux check`.
