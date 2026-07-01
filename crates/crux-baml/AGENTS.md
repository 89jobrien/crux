---
crate: crux-baml
role: baml-integration
constraints:
  - "baml_client/ is generated — never edit"
  - "Cargo.toml baml version must match generators.baml"
  - "Regenerate with: mise exec -- baml-cli generate"
howto:
  - task: "Add a new BAML handler"
    steps:
      - "Define BAML function in .baml files"
      - "Run mise exec -- baml-cli generate"
      - "Create handler wrapper in new .rs file"
      - "Register handler in crux-agentic"
pitfalls:
  - "All tests require API keys"
  - "baml_client/ is gitignored — must regenerate after clone"
commands:
  test: "just sops-run crux dev cargo nextest run -p crux-baml"
---

# Agents: crux-baml

## For AI Agents Working With This Crate

BAML integration layer. Thin wrappers around generated BAML client code.

### Key Constraints

- `baml_client/` is **generated** — never edit files in this directory
- Regenerate with `mise exec -- baml-cli generate` from this crate dir
- Version in `Cargo.toml` (`baml` dep) must match `generators.baml`
