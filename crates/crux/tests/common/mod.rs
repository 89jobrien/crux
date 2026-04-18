/// Shared test fixtures for crux integration tests.
///
/// Import with `mod common;` at the top of any integration test file.
/// All agents here are intentionally minimal — they exist to be delegated to,
/// composed, or used as error-path targets.
use cruxai::prelude::*;

// -- echo ---------------------------------------------------------------------

/// Returns its input unchanged. Useful for trace inspection without side effects.
#[cruxai::agent]
pub async fn echo(msg: String) -> Crux<String> {
    Ok(msg)
}

// -- doubler ------------------------------------------------------------------

/// Doubles its i32 input via a single recorded step.
#[cruxai::agent]
pub async fn doubler(n: i32) -> Crux<i32> {
    let result: i32 = t
        .step("double", || {
            let v = n;
            async move { Ok(v * 2) }
        })
        .await?;
    Ok(result)
}

// -- failer -------------------------------------------------------------------

/// Always fails with a step error. Use as a delegation target to test error paths.
#[cruxai::agent]
pub async fn failer(_input: String) -> Crux<String> {
    Err(CruxErr::step_failed("failer", "always fails"))
}

// -- counter ------------------------------------------------------------------

/// Counts the words in its input string via two sequential steps.
/// Useful for tests that need a multi-step agent with inspectable trace shape.
#[cruxai::agent]
pub async fn counter(text: String) -> Crux<usize> {
    let words: Vec<String> = t
        .step("tokenize", || {
            let t = text.clone();
            async move { Ok(t.split_whitespace().map(str::to_string).collect::<Vec<_>>()) }
        })
        .await?;

    let count: usize = t
        .step("count", || {
            let n = words.len();
            async move { Ok(n) }
        })
        .await?;

    Ok(count)
}
