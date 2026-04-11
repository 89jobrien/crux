/// crux-core: domain types, traits, and runtime for the crux agentic DSL.
pub mod agent;
pub mod context;
pub mod ctx;
pub mod delegation;
pub mod hooks;
pub mod recorder;
pub mod registry;
pub mod replay;
pub mod speculation;
pub mod types;

pub mod prelude {
    pub use crate::agent::Agent;
    pub use crate::context::Context;
    pub use crate::ctx::{BoxFut, ConfidenceRange, ConfidenceRoute, CruxCtx, JoinArm, PipeStage};
    pub use crate::registry::{Task, TaskRegistry, TaskStatus};
    pub use crate::replay::ReplayMode;
    pub use crate::types::budget::Budget;
    pub use crate::types::crux_value::Crux;
    pub use crate::types::error::CruxErr;
    pub use crate::types::id::{CruxId, TaskId};
    pub use crate::types::recovery::Recovery;
    pub use crate::types::step::{Step, StepKind, StepStatus};
}
