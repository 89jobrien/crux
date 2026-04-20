/// Delegation: a parent agent delegates to a child agent with a scoped budget.
///
/// Shows:
/// - `CruxCtx::delegate::<A>(name, input)` fluent builder
/// - `with_budget` to cap the child's token/call budget
/// - Child trace appearing in `cruxx.children`
use cruxx::prelude::*;

// -- Child agent ---------------------------------------------------------------

struct ClassifyAgent;

impl Agent for ClassifyAgent {
    type Input = String;
    type Output = String;

    fn name() -> &'static str {
        "classify"
    }

    async fn run(ctx: &mut CruxCtx, input: Self::Input) -> Result<Self::Output, CruxErr> {
        ctx.step("detect_language", || async move {
            // Simulate classification: count non-ASCII bytes as a rough heuristic.
            let label = if input.is_ascii() {
                "english"
            } else {
                "non-english"
            };
            Ok(label.to_string())
        })
        .await
    }
}

// -- Parent agent --------------------------------------------------------------

struct RouterAgent;

impl Agent for RouterAgent {
    type Input = String;
    type Output = String;

    fn name() -> &'static str {
        "router"
    }

    async fn run(ctx: &mut CruxCtx, input: Self::Input) -> Result<Self::Output, CruxErr> {
        let language = ctx
            .delegate::<ClassifyAgent>("classify_input", input.clone())
            .with_budget(Budget::calls(10))
            .run()
            .await?;

        ctx.step("build_response", || async move {
            Ok(format!(
                "Detected language: {language}. Input was: \"{input}\""
            ))
        })
        .await
    }
}

// -- Main ----------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let input = "Hello from cruxx!".to_string();

    let mut ctx = CruxCtx::new(RouterAgent::name());
    let result = RouterAgent::run(&mut ctx, input).await;
    let cruxx = ctx.finalize(result);

    println!("Agent:  {}", cruxx.agent);
    println!("Result: {:?}", cruxx.value().unwrap());
    println!();
    println!("Parent steps:");
    for step in cruxx.causal_chain() {
        println!("  - {} ({:?})", step.name, step.status);
    }
    println!();
    println!("Child traces: {}", cruxx.children.len());
    for child in &cruxx.children {
        println!("  child agent: {}", child.agent);
        for step in child.causal_chain() {
            println!("    - {} ({:?})", step.name, step.status);
        }
    }
}
