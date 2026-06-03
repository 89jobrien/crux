---
title: SafetyViolation
tags: [type, runtime, safety]
---
# SafetyViolation
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/safety.rs`

Enum (Error trait): HardCapExceeded{resource, limit, proposed},
ForbiddenSyscall{syscall}, Custom{reason}. Returned by [[SafetyPolicy]]::validate().
