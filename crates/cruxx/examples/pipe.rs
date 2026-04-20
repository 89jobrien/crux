/// Pipe: chain sequential transformations, each recorded as a named step.
///
/// Shows:
/// - `CruxCtx::pipe(name, input, stages)` where each stage is a `PipeStage`
/// - Per-stage step naming: `"{pipe_name}::{stage_name}"`
/// - Stages receive the output of the prior stage as input
use cruxx::prelude::*;

#[tokio::main]
async fn main() {
    let raw = "  Hello, World!  This is a cruxx pipeline demo.  ".to_string();

    let mut ctx = CruxCtx::new("text_pipeline");

    let stages: Vec<PipeStage<'_, String>> = vec![
        (
            "trim",
            Box::new(|s: String| -> BoxFut<String> {
                Box::pin(async move { Ok(s.trim().to_string()) })
            }),
        ),
        (
            "lowercase",
            Box::new(|s: String| -> BoxFut<String> {
                Box::pin(async move { Ok(s.to_lowercase()) })
            }),
        ),
        (
            "word_count",
            Box::new(|s: String| -> BoxFut<String> {
                Box::pin(async move {
                    let count = s.split_whitespace().count();
                    Ok(format!("{s} [{count} words]"))
                })
            }),
        ),
    ];

    let result = ctx.pipe("process", raw, stages).await.unwrap();
    let cruxx = ctx.finalize(Ok(result.clone()));

    println!("Output: {result}");
    println!();
    println!("Steps:");
    for step in cruxx.causal_chain() {
        println!("  - {} ({:?})", step.name, step.status);
    }
    println!("Duration: {:?}ms", cruxx.duration_ms());
}
