/// Join-all: fan out to multiple named futures concurrently, collect results.
///
/// Shows:
/// - `CruxCtx::join_all(name, arms)` where each arm is a `JoinArm`
/// - Concurrent execution via `futures::join_all` under the hood
/// - Per-arm step naming: `"{join_name}::{arm_name}"`
/// - Results returned in input order regardless of completion order
use cruxx::prelude::*;

#[tokio::main]
async fn main() {
    let query = "cruxx agentic DSL".to_string();

    let mut ctx = CruxCtx::new("multi_search");

    let arms: Vec<JoinArm<'_, String>> = vec![
        (
            "web",
            Box::pin({
                let q = query.clone();
                async move { Ok(format!("[web] results for '{q}'")) }
            }),
        ),
        (
            "docs",
            Box::pin({
                let q = query.clone();
                async move { Ok(format!("[docs] results for '{q}'")) }
            }),
        ),
        (
            "code",
            Box::pin({
                let q = query.clone();
                async move { Ok(format!("[code] results for '{q}'")) }
            }),
        ),
    ];

    let results = ctx.join_all("search", arms).await.unwrap();
    let combined = results.join("\n");
    let cruxx = ctx.finalize(Ok(combined.clone()));

    println!("Combined results:");
    println!("{combined}");
    println!();
    println!("Steps:");
    for step in cruxx.causal_chain() {
        println!("  - {} ({:?})", step.name, step.status);
    }
}
