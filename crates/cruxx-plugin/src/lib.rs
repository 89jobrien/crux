//! cruxx-plugin -- subprocess plugin host for cruxx pipelines.
//!
//! Plugins are external binaries speaking a JSON-RPC protocol over
//! stdin/stdout. The host discovers them from a manifest, launches
//! them as persistent child processes, and bridges their handlers
//! into the cruxx `HandlerRegistry`.

pub mod bridge;
pub mod host;
pub mod manifest;
pub mod protocol;
