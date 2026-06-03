---
title: GovernancePolicy
tags: [type, runtime, governance]
---
# GovernancePolicy
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/governance.rs`

Composable policy with allowed_tools, blocked_tools, blocked_patterns,
max_calls_per_request (default 100), require_human_approval list.
Methods: `check_tool(name)` → [[PolicyAction]], `check_content(content)` → Option<String>.
`compose_policies()` unions blocked, intersects allowed, takes min rate limits.
