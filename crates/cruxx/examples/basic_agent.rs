/// A minimal cruxx agent that demonstrates step recording, delegation, and trace inspection.
use cruxx::prelude::*;

// -- Agents -------------------------------------------------------------------

struct SummarizeAgent;

impl Agent for SummarizeAgent {
    type Input = String;
    type Output = String;

    fn name() -> &'static str {
        "summarize"
    }

    async fn run(ctx: &mut CruxCtx, input: Self::Input) -> Result<Self::Output, CruxErr> {
        let word_count: usize = ctx
            .step("count_words", || async move {
                Ok(input.split_whitespace().count())
            })
            .await?;

        ctx.step("format", || async move {
            Ok(format!("The input has {word_count} words."))
        })
        .await
    }
}

// -- Main ---------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let input = "cruxx makes agentic control flow explicit in the type system".to_string();

    // Run the agent manually (without the proc macro, for clarity).
    let mut ctx = CruxCtx::new(SummarizeAgent::name());
    let result = SummarizeAgent::run(&mut ctx, input).await;
    let cruxx = ctx.finalize(result);

    // Inspect the trace.
    println!("Agent: {}", cruxx.agent);
    println!("Result: {:?}", cruxx.value().unwrap());
    println!("Steps:");
    for step in cruxx.causal_chain() {
        println!(
            "  - {} ({:?}, confidence={:.1})",
            step.name, step.status, step.confidence
        );
    }
    println!("Duration: {:?}ms", cruxx.duration_ms());
}
