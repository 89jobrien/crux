//! PlannerGate — wires a Planner into step execution.
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use cruxx_domain::planner::{DenyAllPlanner, PassthroughPlanner, SimulatePlanner};

    use crate::context::Context as _;
    use crate::ctx::CruxCtx;
    use crate::types::error::CruxErr;

    #[tokio::test]
    async fn passthrough_planner_executes_step() {
        let mut ctx = CruxCtx::new("agent");
        ctx.set_planner(PassthroughPlanner);
        let result = ctx.step("a", || async { Ok::<i32, CruxErr>(1) }).await;
        assert_eq!(result.unwrap(), 1);
    }

    #[tokio::test]
    async fn deny_planner_fails_step_with_denied_error() {
        let mut ctx = CruxCtx::new("agent");
        ctx.set_planner(DenyAllPlanner {
            reason: "blocked".into(),
        });
        let result = ctx.step("a", || async { Ok::<i32, CruxErr>(1) }).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("blocked"),
            "expected 'blocked' in: {err}"
        );
    }

    #[tokio::test]
    async fn simulate_planner_returns_synthetic_output_without_running_closure() {
        let ran = Arc::new(AtomicBool::new(false));
        let ran2 = ran.clone();

        let mut ctx = CruxCtx::new("agent");
        ctx.set_planner(SimulatePlanner {
            output: serde_json::json!(99),
        });

        let result = ctx
            .step("a", || async move {
                ran2.store(true, Ordering::SeqCst);
                Ok::<i32, CruxErr>(1)
            })
            .await;

        assert!(!ran.load(Ordering::SeqCst), "closure should not have run");
        assert_eq!(result.unwrap(), 99i32);
    }
}
