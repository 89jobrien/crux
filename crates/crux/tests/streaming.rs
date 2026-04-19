/// Integration tests for step_stream (streaming/incremental steps).
use cruxai::prelude::*;
use futures::stream;

#[cruxai::agent]
async fn streaming_agent(items: Vec<i32>) -> Crux<i32> {
    let sum: i32 = x
        .step_stream("accumulate", || stream::iter(items.into_iter().map(Ok)))
        .await?;
    Ok(sum)
}

#[tokio::test]
async fn step_stream_collects_events() {
    let crux = streaming_agent(vec![1, 2, 3]).await;
    assert_eq!(crux.value().unwrap(), &3); // last item is the output
    assert_eq!(crux.steps.len(), 1);
    assert_eq!(crux.steps[0].name, "accumulate");
    assert!(crux.steps[0].is_ok());
    // Events contain all yielded items (including final)
    assert_eq!(crux.steps[0].events.len(), 3);
    assert_eq!(crux.steps[0].events[0], serde_json::json!(1));
    assert_eq!(crux.steps[0].events[1], serde_json::json!(2));
    assert_eq!(crux.steps[0].events[2], serde_json::json!(3));
}

#[cruxai::agent]
async fn streaming_fails(items: Vec<i32>) -> Crux<i32> {
    let val: i32 = x
        .step_stream("partial", || {
            stream::iter(items.into_iter().map(|i| {
                if i < 0 {
                    Err(CruxErr::step_failed("partial", "negative value"))
                } else {
                    Ok(i)
                }
            }))
        })
        .await?;
    Ok(val)
}

#[tokio::test]
async fn step_stream_fails_on_error_item() {
    let crux = streaming_fails(vec![1, 2, -1, 3]).await;
    assert!(crux.value().is_err());
    assert_eq!(crux.steps.len(), 1);
    assert!(crux.steps[0].is_err());
    // Events collected before the error
    assert_eq!(crux.steps[0].events.len(), 2);
    assert_eq!(crux.steps[0].events[0], serde_json::json!(1));
    assert_eq!(crux.steps[0].events[1], serde_json::json!(2));
}

#[cruxai::agent]
async fn streaming_empty() -> Crux<i32> {
    let val: i32 = x
        .step_stream("empty", || {
            stream::iter(std::iter::empty::<Result<i32, CruxErr>>())
        })
        .await?;
    Ok(val)
}

#[tokio::test]
async fn step_stream_empty_fails() {
    let crux = streaming_empty().await;
    assert!(crux.value().is_err());
}

#[tokio::test]
async fn step_stream_events_serialize_in_trace() {
    let crux = streaming_agent(vec![10, 20]).await;
    let json = serde_json::to_string(&crux).unwrap();
    assert!(json.contains("\"events\""));
    // Round-trip
    let back: Crux<i32> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.steps[0].events.len(), 2);
}
