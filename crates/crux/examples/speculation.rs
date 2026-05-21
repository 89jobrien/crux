/// Speculation: run multiple approaches and pick the best result.
///
/// Shows:
/// - `CruxCtx::speculate(name, arms)` where arms are `(&str, BoxFut<T>)` pairs
/// - `pick_best_by` — scores all successful arms, takes the highest
/// - `first_ok` — returns the first arm that succeeds
/// - Winner recorded as `StepStatus::Ok`, losers as `Rejected`
use crux::prelude::*;

#[tokio::main]
async fn main() {
    // -- pick_best_by ----------------------------------------------------------

    let mut ctx = CruxCtx::new("pick_best_demo");

    let arms: Vec<(&str, BoxFut<String>)> = vec![
        (
            "short_summary",
            Box::pin(async { Ok("Brief.".to_string()) }),
        ),
        (
            "detailed_summary",
            Box::pin(async { Ok("A much more detailed summary of the input.".to_string()) }),
        ),
        (
            "medium_summary",
            Box::pin(async { Ok("A moderate summary.".to_string()) }),
        ),
    ];

    let best = ctx
        .speculate("summarize", arms)
        .pick_best_by(|s: &String| s.len() as f32)
        .await
        .unwrap();
    let crux = ctx.finalize(Ok(best.clone()));

    println!("=== pick_best_by ===");
    println!("Winner: {best:?}");
    println!("Steps:");
    for step in crux.causal_chain() {
        println!("  - {} ({:?})", step.name, step.status);
    }
    println!();

    // -- first_ok --------------------------------------------------------------

    let mut ctx2 = CruxCtx::new("first_ok_demo");

    let arms2: Vec<(&str, BoxFut<String>)> = vec![
        (
            "primary",
            Box::pin(async {
                Err(CruxErr::step_failed(
                    "primary",
                    "primary source unavailable",
                ))
            }),
        ),
        (
            "fallback",
            Box::pin(async { Ok("Fallback result.".to_string()) }),
        ),
        (
            "secondary_fallback",
            Box::pin(async { Ok("This would never run.".to_string()) }),
        ),
    ];

    let first = ctx2.speculate("fetch", arms2).first_ok().await.unwrap();
    let crux2 = ctx2.finalize(Ok(first.clone()));

    println!("=== first_ok ===");
    println!("First success: {first:?}");
    println!("Steps:");
    for step in crux2.causal_chain() {
        println!("  - {} ({:?})", step.name, step.status);
    }
}
