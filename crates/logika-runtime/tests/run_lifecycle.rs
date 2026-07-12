//! Run lifecycle model integration tests.

#![allow(clippy::expect_used)]

use std::time::SystemTime;

use logika_core::RunId;
use logika_runtime::{Run, RunOptions, RunOutcome, RunStatus};
use semver::Version;
use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

mod support;

#[test]
fn run_captures_plan_identity_and_host_metadata() {
    let plan = support::plan();
    let run_id = RunId::new("run-2026-07-12").expect("test run id must be valid");
    let started_at = SystemTime::UNIX_EPOCH;
    let deadline = Instant::now() + Duration::from_secs(30);
    let cancellation = CancellationToken::new();
    let run = Run::new_at(
        run_id.clone(),
        &plan,
        RunOptions::default()
            .with_correlation_id("request-42")
            .with_deadline(deadline)
            .with_cancellation_token(cancellation.clone()),
        started_at,
    );

    assert_eq!(run.metadata().run_id(), &run_id);
    assert_eq!(run.metadata().workflow_name(), "runtime-model");
    assert_eq!(run.metadata().workflow_version(), &Version::new(2, 3, 4));
    assert_eq!(run.metadata().plan_hash(), plan.plan_hash());
    assert_eq!(run.metadata().started_at(), started_at);
    assert_eq!(run.metadata().correlation_id(), "request-42");
    assert_eq!(run.deadline(), Some(deadline));
    assert_eq!(run.status(), RunStatus::Running);
    assert_eq!(run.finished_at(), None);

    cancellation.cancel();
    assert!(run.cancellation_token().is_cancelled());
}

#[test]
fn lifecycle_allows_exactly_one_terminal_transition() {
    let plan = support::plan();
    let run_id = RunId::new("run-terminal").expect("test run id must be valid");
    let mut run = Run::new_at(run_id, &plan, RunOptions::default(), SystemTime::UNIX_EPOCH);
    let finished_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);

    run.finish_at(RunOutcome::Succeeded, finished_at)
        .expect("running run must accept a terminal outcome");

    assert_eq!(run.status(), RunStatus::Succeeded);
    assert_eq!(run.finished_at(), Some(finished_at));
    assert!(run.status().is_terminal());

    let error = run
        .finish(RunOutcome::Failed)
        .expect_err("terminal run must reject a second outcome");
    assert_eq!(error.current(), RunStatus::Succeeded);
    assert_eq!(error.requested(), RunStatus::Failed);
    assert_eq!(run.status(), RunStatus::Succeeded);
    assert_eq!(run.finished_at(), Some(finished_at));
}

#[test]
fn default_options_are_unbounded_and_independently_cancellable() {
    let first = RunOptions::default();
    let second = RunOptions::default();

    assert_eq!(first.correlation_id(), None);
    assert_eq!(first.deadline(), None);
    first.cancellation_token().cancel();

    assert!(first.cancellation_token().is_cancelled());
    assert!(!second.cancellation_token().is_cancelled());
}

#[test]
fn run_id_is_the_default_correlation_id() {
    let plan = support::plan();
    let run = Run::new(
        RunId::new("run-correlated").expect("test run id must be valid"),
        &plan,
        RunOptions::default(),
    );

    assert_eq!(run.metadata().correlation_id(), "run-correlated");
}
