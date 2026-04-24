//! cruxx-domain: Pure domain types for the cruxx agentic DSL.
//!
//! Zero async, zero LLM dependencies. External consumers (minibox, slash)
//! can depend on this crate without pulling tokio or BAML.

pub mod action;
pub mod plan_result;
pub mod planner;

#[cfg(test)]
mod tests {
    use crate::action::{Action, StepIntent};
    use crate::plan_result::PlanResult;

    use crate::planner::{PassthroughPlanner, Planner};

    #[test]
    fn domain_crate_compiles() {}

    #[test]
    fn action_execute_roundtrips_serde() {
        let a = Action::Execute(StepIntent {
            name: "my_step".into(),
            priority: 0,
        });
        let json = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Action::Execute(_)));
    }

    #[test]
    fn plan_result_allow_carries_action() {
        let a = Action::Execute(StepIntent { name: "x".into(), priority: 0 });
        let r = PlanResult::Allow(a.clone());
        assert!(matches!(r, PlanResult::Allow(_)));
    }

    #[test]
    fn plan_result_deny_carries_reason() {
        let r = PlanResult::Deny { reason: "unsafe".into() };
        if let PlanResult::Deny { reason } = r {
            assert_eq!(reason, "unsafe");
        }
    }

    #[test]
    fn plan_result_simulate_carries_output() {
        let r = PlanResult::Simulate { output: serde_json::json!(42) };
        if let PlanResult::Simulate { output } = r {
            assert_eq!(output, serde_json::json!(42));
        }
    }

    #[test]
    fn passthrough_allows_all_steps() {
        let p = PassthroughPlanner;
        let result = p.next_action("my_step", 0);
        assert!(matches!(result, PlanResult::Allow(_)));
    }

    #[test]
    fn passthrough_preserves_step_name() {
        let p = PassthroughPlanner;
        if let PlanResult::Allow(action) = p.next_action("fetch_data", 0) {
            assert_eq!(action.name(), "fetch_data");
        } else {
            panic!("expected Allow");
        }
    }
}
