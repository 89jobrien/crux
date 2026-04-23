//! Compile-time constants for all built-in handler names.
//!
//! Use these instead of raw string literals to catch typos at compile time.

// shell
pub const SHELL_EXEC: &str = "shell::exec";
pub const SHELL_CAPTURE: &str = "shell::capture";

// fs
pub const FS_READ: &str = "fs::read";
pub const FS_WRITE: &str = "fs::write";
pub const FS_GLOB: &str = "fs::glob";
pub const FS_EXISTS: &str = "fs::exists";

// git
pub const GIT_STAGED_FILES: &str = "git::staged_files";
pub const GIT_DIFF: &str = "git::diff";
pub const GIT_LOG: &str = "git::log";
pub const GIT_STATUS: &str = "git::status";

// json
pub const JSON_PICK: &str = "json::pick";
pub const JSON_MERGE: &str = "json::merge";
pub const JSON_JQ: &str = "json::jq";

// ctrl
pub const CTRL_NOOP: &str = "ctrl::noop";
pub const CTRL_LOG: &str = "ctrl::log";
pub const CTRL_ASSERT: &str = "ctrl::assert";

// llm
pub const LLM_INVOKE: &str = "llm::invoke";
pub const LLM_EXTRACT: &str = "llm::extract";
pub const LLM_DECOMPOSE: &str = "llm::decompose";
pub const LLM_PLAN: &str = "llm::plan";
pub const LLM_STREAM: &str = "llm::stream";

// container / harness
pub const CONTAINER_RUN: &str = "container::run";
pub const HARNESS_RUN: &str = "harness::run";

// rx
pub const RX_RUN: &str = "rx::run";
pub const RX_LIST: &str = "rx::list";

// sqlite
pub const SQLITE_EXEC: &str = "sqlite::exec";
pub const SQLITE_QUERY_ONE: &str = "sqlite::query_one";
pub const SQLITE_QUERY_MANY: &str = "sqlite::query_many";
pub const SQLITE_INSERT: &str = "sqlite::insert";
pub const SQLITE_UPDATE: &str = "sqlite::update";
pub const SQLITE_DELETE: &str = "sqlite::delete";
pub const SQLITE_UPSERT: &str = "sqlite::upsert";
