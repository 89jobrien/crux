//! Runtime-only typed pipeline intermediate representation.

use std::{fmt, sync::Arc};

use crate::schema::{BudgetDef, PipelineDef, PipelineDisplayDef, StepNode};
use crate::step_runner::StepRunner;

#[derive(Clone)]
pub(crate) struct TypedStep {
    pub(crate) node: StepNode,
    pub(crate) runner: Arc<dyn StepRunner>,
}

impl fmt::Debug for TypedStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedStep")
            .field("name", &self.node.step)
            .field("handler", &self.runner.metadata().name)
            .finish_non_exhaustive()
    }
}

/// Pipeline definition whose simple handler names have been resolved to executors.
#[derive(Clone)]
pub struct TypedPipeline {
    pub(crate) name: String,
    pub(crate) steps: Vec<TypedStep>,
    pub(crate) budget: Option<BudgetDef>,
    pub(crate) display: Option<PipelineDisplayDef>,
}

impl TypedPipeline {
    pub(crate) fn new(definition: &PipelineDef, steps: Vec<TypedStep>) -> Self {
        Self {
            name: definition.pipeline.clone(),
            steps,
            budget: definition.budget.clone(),
            display: definition.display.clone(),
        }
    }

    /// Return the stable pipeline name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the number of compiled top-level steps.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

impl fmt::Debug for TypedPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedPipeline")
            .field("name", &self.name)
            .field("steps", &self.steps)
            .field("budget", &self.budget)
            .field("display", &self.display)
            .finish()
    }
}
