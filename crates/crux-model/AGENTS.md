---
crate: crux-model
role: parser
howto:
  - task: "Add a new provider"
    steps:
      - "Add variant to Vendor enum in vendor.rs"
      - "Add detection patterns in vendor.rs"
      - "Add parser rules in parser/"
      - "Add test cases for new provider model strings"
      - "Update canonical.rs if unusual versioning"
pitfalls:
  - "Do not change normalization without checking downstream consumers"
  - "Test with real model strings from provider API docs"
---

# Agents: crux-model

## For AI Agents Working With This Crate

Model ID normalization. Purely parsing and data types.

### Common Pitfalls

- Do not change model name normalization without checking downstream
  consumers (routing, billing, logging all depend on canonical IDs)
- Test with real model strings from the provider's API docs
