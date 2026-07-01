---
crate: crux-agentic
total: 2
by_status:
  open: 2
  done: 0
items:
  - id: null
    status: open
    priority: low
    area: performance
    title: "Take &str for api_key in fallback handler"
    location: "src/llm.rs::parse_llm_input:63"
  - id: null
    status: open
    priority: low
    area: correctness
    title: "Verify RiskLevel discriminant mapping"
    location: "src/adapters/terminal_approval.rs::new:15"
---

# TODO: crux-agentic

- [ ] `src/llm.rs::parse_llm_input:63` — consider taking `&str` for
  `api_key` instead of cloning in the fallback handler
- [ ] `src/adapters/terminal_approval.rs::new:15` — verify `RiskLevel`
  discriminants match old Low->1..Critical->4 mapping
