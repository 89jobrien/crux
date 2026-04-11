/// crux-core: domain types, traits, and runtime for the crux agentic DSL.
pub mod agent;
pub mod ctx;
pub mod registry;
pub mod types;

pub mod prelude {
    pub use crate::agent::Agent;
    pub use crate::ctx::CruxCtx;
    pub use crate::types::budget::Budget;
    pub use crate::types::crux_value::Crux;
    pub use crate::types::error::CruxErr;
    pub use crate::types::id::{CruxId, TaskId};
    pub use crate::types::recovery::Recovery;
    pub use crate::types::step::{Step, StepKind, StepStatus};
}
