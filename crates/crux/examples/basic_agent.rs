/// A minimal crux agent that demonstrates step recording, delegation, and trace inspection.
use cruxai::prelude::*;

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
    let input = "crux makes agentic control flow explicit in the type system".to_string();

    // Run the agent manually (without the proc macro, for clarity).
    let mut ctx = CruxCtx::new(SummarizeAgent::name());
    let result = SummarizeAgent::run(&mut ctx, input).await;
    let crux = ctx.finalize(result);

    // Inspect the trace.
    println!("Agent: {}", crux.agent);
    println!("Result: {:?}", crux.value().unwrap());
    println!("Steps:");
    for step in crux.causal_chain() {
        println!(
            "  - {} ({:?}, confidence={:.1})",
            step.name, step.status, step.confidence
        );
    }
    println!("Duration: {:?}ms", crux.duration_ms());
}
