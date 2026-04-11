/// Integration tests for t.speculate() and SpeculationBuilder.
use crux::prelude::*;

// -- pick_best_by -----------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Scored {
    value: String,
    score: f32,
}

#[tokio::test]
async fn pick_best_by_selects_highest() {
    let mut ctx = CruxCtx::new("test");

    let result: Scored = ctx
        .speculate(
            "choose",
            vec![
                (
                    "low",
                    Box::pin(async {
                        Ok(Scored {
                            value: "low".into(),
                            score: 0.3,
                        })
                    }),
                ),
                (
                    "high",
                    Box::pin(async {
                        Ok(Scored {
                            value: "high".into(),
                            score: 0.9,
                        })
                    }),
                ),
                (
                    "mid",
                    Box::pin(async {
                        Ok(Scored {
                            value: "mid".into(),
                            score: 0.6,
                        })
                    }),
                ),
            ],
        )
        .pick_best_by(|r| r.score)
        .await
        .unwrap();

    assert_eq!(result.value, "high");
    assert_eq!(result.score, 0.9);

    // Winner is Ok, losers are Rejected
    let steps = ctx.snapshot_steps();
    assert_eq!(steps.len(), 3);

    let ok_steps: Vec<_> = steps.iter().filter(|s| s.is_ok()).collect();
    assert_eq!(ok_steps.len(), 1);
    assert!(ok_steps[0].name.contains("high"));

    let rejected: Vec<_> = steps
        .iter()
        .filter(|s| s.status == StepStatus::Rejected)
        .collect();
    assert_eq!(rejected.len(), 2);
}

// -- pick_best_by with some failures ----------------------------------------

#[tokio::test]
async fn pick_best_by_skips_failures() {
    let mut ctx = CruxCtx::new("test");

    let result: i32 = ctx
        .speculate(
            "partial",
            vec![
                (
                    "fail",
                    Box::pin(async { Err(CruxErr::step_failed("fail", "nope")) }),
                ),
                ("ok", Box::pin(async { Ok(42) })),
            ],
        )
        .pick_best_by(|&v| v as f32)
        .await
        .unwrap();

    assert_eq!(result, 42);
}

// -- pick_best_by all fail --------------------------------------------------

#[tokio::test]
async fn pick_best_by_all_fail() {
    let mut ctx = CruxCtx::new("test");

    let result: Result<i32, _> = ctx
        .speculate(
            "all_fail",
            vec![
                (
                    "a",
                    Box::pin(async { Err(CruxErr::step_failed("a", "nope")) }),
                ),
                (
                    "b",
                    Box::pin(async { Err(CruxErr::step_failed("b", "nope")) }),
                ),
            ],
        )
        .pick_best_by(|&v| v as f32)
        .await;

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("all speculation arms failed"));
}

// -- first_ok ---------------------------------------------------------------

#[tokio::test]
async fn first_ok_returns_first_success() {
    let mut ctx = CruxCtx::new("test");

    let result: i32 = ctx
        .speculate(
            "first",
            vec![
                (
                    "fail",
                    Box::pin(async { Err(CruxErr::step_failed("fail", "nope")) }),
                ),
                ("ok", Box::pin(async { Ok(7) })),
                ("also_ok", Box::pin(async { Ok(99) })),
            ],
        )
        .first_ok()
        .await
        .unwrap();

    assert_eq!(result, 7);

    // First arm rejected, second succeeded, third never ran
    let steps = ctx.snapshot_steps();
    assert_eq!(steps.len(), 2); // fail + ok
}

// -- first_ok all fail ------------------------------------------------------

#[tokio::test]
async fn first_ok_all_fail() {
    let mut ctx = CruxCtx::new("test");

    let result: Result<i32, _> = ctx
        .speculate(
            "none",
            vec![
                (
                    "a",
                    Box::pin(async { Err(CruxErr::step_failed("a", "nope")) }),
                ),
                (
                    "b",
                    Box::pin(async { Err(CruxErr::step_failed("b", "nope")) }),
                ),
            ],
        )
        .first_ok()
        .await;

    assert!(result.is_err());
}

// -- speculation steps have Speculation kind ---------------------------------

#[tokio::test]
async fn speculation_steps_have_correct_kind() {
    let mut ctx = CruxCtx::new("test");

    let _: i32 = ctx
        .speculate(
            "kind_check",
            vec![
                ("a", Box::pin(async { Ok(1) })),
                ("b", Box::pin(async { Ok(2) })),
            ],
        )
        .pick_best_by(|&v| v as f32)
        .await
        .unwrap();

    for step in ctx.snapshot_steps() {
        assert_eq!(step.kind, StepKind::Speculation);
    }
}

// -- speculation inside #[crux::agent] --------------------------------------

#[crux::agent]
async fn speculative_agent(input: i32) -> Crux<i32> {
    let result: i32 = t
        .speculate(
            "pick",
            vec![
                ("double", Box::pin(async move { Ok(input * 2) })),
                ("triple", Box::pin(async move { Ok(input * 3) })),
            ],
        )
        .pick_best_by(|&v| v as f32)
        .await?;
    Ok(result)
}

#[tokio::test]
async fn speculation_through_macro() {
    let crux = speculative_agent(5).await;
    // Triple (15) should win over double (10)
    assert_eq!(crux.value().unwrap(), &15);
}
