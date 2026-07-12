//! Node execution context integration tests.

#![allow(clippy::expect_used)]

use std::{num::NonZeroU32, time::SystemTime};

use logika_core::{NodeId, RunId};
use logika_runtime::{Run, RunOptions};
use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

mod support;

#[test]
fn node_context_carries_attempt_and_immutable_run_metadata() {
    let plan = support::plan();
    let deadline = Instant::now() + Duration::from_secs(60);
    let cancellation = CancellationToken::new();
    let run = Run::new_at(
        RunId::new("run-context").expect("test run id must be valid"),
        &plan,
        RunOptions::default()
            .with_correlation_id("trace-7")
            .with_deadline(deadline)
            .with_cancellation_token(cancellation),
        SystemTime::UNIX_EPOCH,
    );
    let node_id = NodeId::new("enrich").expect("test node id must be valid");
    let context = run.node_context(
        node_id.clone(),
        NonZeroU32::new(2).expect("two is non-zero"),
    );

    assert_eq!(context.node_id(), &node_id);
    assert_eq!(context.attempt().get(), 2);
    assert_eq!(context.run().run_id(), run.metadata().run_id());
    assert_eq!(context.run().plan_hash(), plan.plan_hash());
    assert_eq!(context.run().correlation_id(), "trace-7");
    assert_eq!(context.deadline(), Some(deadline));
    assert!(!context.deadline_exceeded());
    assert!(
        context
            .remaining()
            .is_some_and(|remaining| !remaining.is_zero())
    );
}

#[test]
fn cloned_contexts_observe_external_cancellation() {
    let plan = support::plan();
    let cancellation = CancellationToken::new();
    let run = Run::new(
        RunId::new("run-cancel").expect("test run id must be valid"),
        &plan,
        RunOptions::default().with_cancellation_token(cancellation.clone()),
    );
    let context = run.node_context(
        NodeId::new("fetch").expect("test node id must be valid"),
        NonZeroU32::MIN,
    );
    let cloned = context.clone();

    cancellation.cancel();

    assert!(context.is_cancelled());
    assert!(cloned.is_cancelled());
    assert!(context.cancellation_token().is_cancelled());
}

#[test]
fn elapsed_deadline_is_visible_without_implying_cancellation() {
    let plan = support::plan();
    let run = Run::new(
        RunId::new("run-deadline").expect("test run id must be valid"),
        &plan,
        RunOptions::default().with_deadline(Instant::now() - Duration::from_millis(1)),
    );
    let context = run.node_context(
        NodeId::new("slow").expect("test node id must be valid"),
        NonZeroU32::MIN,
    );

    assert!(context.deadline_exceeded());
    assert_eq!(context.remaining(), Some(Duration::ZERO));
    assert!(!context.is_cancelled());
}
