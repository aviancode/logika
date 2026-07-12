//! Runtime retry, timeout, cancellation, and idempotency integration tests.

#![allow(clippy::expect_used)]

use std::{
    collections::BTreeMap,
    future::pending,
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use logika_core::{
    Error as NodeError, ErrorCategory, ErrorDetail, NodeId, Payload, PortId, PrimitiveType, RunId,
    SchemaDefinition, TypeRef,
};
use logika_runtime::{
    ATTEMPT_TIMEOUT_CODE, ExecutionError, NodeExecution, NodeExecutor, NodeFuture, NodeOutputs,
    Run, RunOptions, RunStatus, Scheduler, SchedulerConfig, WorkflowInputs,
};
use logika_workflow::{
    CompilationOptions, CompilationResolver, EdgeDefinition, Endpoint, ExecutionPlan,
    ExecutionPolicy, InputPort, NodeDefinition, NodeInterface, NodeReference, TypeReference,
    ValidationResolver, WorkflowDocument, WorkflowMetadata, WorkflowSpec, compile_workflow,
};
use semver::Version;
use tokio::{runtime::Builder, sync::oneshot, time::Instant};
use tokio_util::sync::CancellationToken;

#[test]
fn retries_only_classified_errors_and_rotates_attempt_keys() {
    runtime().block_on(async {
        let policy = ExecutionPolicy::retry(non_zero_u32(3))
            .with_backoff(Duration::ZERO, Duration::ZERO, Duration::ZERO)
            .with_retriable_code("test.transient");
        let plan = plan(policy);
        let mut run = test_run("retry-run", &plan, RunOptions::default());
        let executor = Arc::new(ScriptedExecutor::new(Action::FailThenSucceed {
            failures: 2,
            code: "test.transient",
        }));

        let result = scheduler()
            .execute(&plan, &mut run, inputs("ready"), executor.clone())
            .await
            .expect("classified transient failures must be retried");

        assert_eq!(run.status(), RunStatus::Succeeded);
        assert_eq!(executor.attempts(), 3);
        assert_eq!(
            executor.keys(),
            ["retry-run/work/1", "retry-run/work/2", "retry-run/work/3"]
        );
        assert_eq!(result.outputs().get(&port("result")), Some(&text("ready")));
    });
}

#[test]
fn does_not_retry_an_unclassified_error() {
    runtime().block_on(async {
        let policy = ExecutionPolicy::retry(non_zero_u32(4))
            .with_backoff(Duration::ZERO, Duration::ZERO, Duration::ZERO)
            .with_retriable_code("test.transient");
        let plan = plan(policy);
        let mut run = test_run("fail-fast", &plan, RunOptions::default());
        let executor = Arc::new(ScriptedExecutor::new(Action::FailThenSucceed {
            failures: 1,
            code: "test.permanent",
        }));

        let error = scheduler()
            .execute(&plan, &mut run, inputs("value"), executor.clone())
            .await
            .expect_err("an unclassified error must fail immediately");

        assert!(matches!(
            error,
            ExecutionError::NodeFailed { source, .. } if source.code() == "test.permanent"
        ));
        assert_eq!(executor.attempts(), 1);
        assert_eq!(run.status(), RunStatus::Failed);
    });
}

#[test]
fn applies_exponential_backoff_between_retries() {
    runtime().block_on(async {
        let policy = ExecutionPolicy::retry(non_zero_u32(3))
            .with_backoff(
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::ZERO,
            )
            .with_retriable_code("test.transient");
        let plan = plan(policy);
        let mut run = test_run("backoff-run", &plan, RunOptions::default());
        let executor = Arc::new(ScriptedExecutor::new(Action::FailThenSucceed {
            failures: 2,
            code: "test.transient",
        }));
        let started = Instant::now();

        scheduler()
            .execute(&plan, &mut run, inputs("ready"), executor)
            .await
            .expect("retries must eventually succeed");

        assert!(started.elapsed() >= Duration::from_millis(30));
    });
}

#[test]
fn retries_attempt_timeouts_without_exceeding_the_plan_limit() {
    runtime().block_on(async {
        let timeout = Duration::from_millis(10);
        let policy = ExecutionPolicy::retry(non_zero_u32(2))
            .with_backoff(Duration::ZERO, Duration::ZERO, Duration::ZERO)
            .with_attempt_timeout(timeout)
            .with_retriable_code(ATTEMPT_TIMEOUT_CODE);
        let plan = plan(policy);
        let mut run = test_run("attempt-timeout", &plan, RunOptions::default());
        let executor = Arc::new(ScriptedExecutor::new(Action::Sleep(Duration::from_millis(
            100,
        ))));

        let error = scheduler()
            .execute(&plan, &mut run, inputs("slow"), executor.clone())
            .await
            .expect_err("both attempts must time out");

        assert!(matches!(
            error,
            ExecutionError::AttemptTimedOut {
                attempt,
                timeout: actual,
                ..
            } if attempt.get() == 2 && actual == timeout
        ));
        assert_eq!(executor.attempts(), 2);
        assert_eq!(run.status(), RunStatus::Failed);
    });
}

#[test]
fn enforces_the_run_deadline_and_cancels_the_node_context() {
    runtime().block_on(async {
        let plan = plan(ExecutionPolicy::single_attempt());
        let options =
            RunOptions::default().with_deadline(Instant::now() + Duration::from_millis(20));
        let mut run = test_run("run-timeout", &plan, options);
        let executor = Arc::new(ScriptedExecutor::new(Action::Sleep(Duration::from_secs(1))));

        let error = scheduler()
            .execute(&plan, &mut run, inputs("slow"), executor.clone())
            .await
            .expect_err("the complete run must respect its deadline");

        assert!(matches!(error, ExecutionError::RunTimedOut));
        assert_eq!(run.status(), RunStatus::TimedOut);
        assert!(run.cancellation_token().is_cancelled());
        assert!(executor.tokens_are_cancelled());
    });
}

#[test]
fn external_cancellation_finishes_pending_nodes() {
    runtime().block_on(async {
        let plan = plan(ExecutionPolicy::single_attempt());
        let cancellation = CancellationToken::new();
        let mut run = test_run(
            "cancel-run",
            &plan,
            RunOptions::default().with_cancellation_token(cancellation.clone()),
        );
        let (started_tx, started_rx) = oneshot::channel();
        let executor = Arc::new(ScriptedExecutor::pending(started_tx));
        let cancel = tokio::spawn(async move {
            started_rx
                .await
                .expect("node must start before cancellation");
            cancellation.cancel();
        });

        let error = scheduler()
            .execute(&plan, &mut run, inputs("pending"), executor.clone())
            .await
            .expect_err("external cancellation must stop a pending node");
        cancel.await.expect("cancellation task must finish");

        assert!(matches!(error, ExecutionError::Cancelled));
        assert_eq!(run.status(), RunStatus::Cancelled);
        assert_eq!(executor.attempts(), 1);
        assert!(executor.tokens_are_cancelled());
    });
}

enum Action {
    FailThenSucceed { failures: u32, code: &'static str },
    Sleep(Duration),
    Pending(Mutex<Option<oneshot::Sender<()>>>),
}

struct ScriptedExecutor {
    action: Action,
    attempts: AtomicU32,
    keys: Mutex<Vec<String>>,
    tokens: Mutex<Vec<CancellationToken>>,
}

impl ScriptedExecutor {
    fn new(action: Action) -> Self {
        Self {
            action,
            attempts: AtomicU32::new(0),
            keys: Mutex::new(Vec::new()),
            tokens: Mutex::new(Vec::new()),
        }
    }

    fn pending(started: oneshot::Sender<()>) -> Self {
        Self::new(Action::Pending(Mutex::new(Some(started))))
    }

    fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }

    fn keys(&self) -> Vec<String> {
        self.keys.lock().expect("keys lock must be healthy").clone()
    }

    fn tokens_are_cancelled(&self) -> bool {
        let tokens = self.tokens.lock().expect("tokens lock must be healthy");
        !tokens.is_empty() && tokens.iter().all(CancellationToken::is_cancelled)
    }
}

impl NodeExecutor for ScriptedExecutor {
    fn execute(&self, execution: NodeExecution) -> NodeFuture {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        self.keys
            .lock()
            .expect("keys lock must be healthy")
            .push(execution.context().idempotency_key().to_owned());
        self.tokens
            .lock()
            .expect("tokens lock must be healthy")
            .push(execution.context().cancellation_token());
        let input = execution.inputs()[&port("in")][0].clone();

        match &self.action {
            Action::FailThenSucceed { failures, code } if attempt <= *failures => {
                let code = *code;
                Box::pin(async move {
                    Err(NodeError::new(
                        ErrorCategory::UserNode,
                        ErrorDetail::new(code, "scripted node failure"),
                    ))
                })
            }
            Action::Sleep(delay) => {
                let delay = *delay;
                Box::pin(async move {
                    tokio::time::sleep(delay).await;
                    Ok(NodeOutputs::from([(port("out"), input)]))
                })
            }
            Action::Pending(started) => {
                if let Some(started) = started.lock().expect("started lock must be healthy").take()
                {
                    let _ = started.send(());
                }
                Box::pin(async move {
                    pending::<()>().await;
                    Ok(NodeOutputs::from([(port("out"), input)]))
                })
            }
            Action::FailThenSucceed { .. } => {
                Box::pin(async move { Ok(NodeOutputs::from([(port("out"), input)])) })
            }
        }
    }
}

struct Resolver {
    payload: TypeRef,
    interface: NodeInterface,
    version: Version,
}

impl ValidationResolver for Resolver {
    fn resolve_node(&self, reference: &NodeReference) -> Option<&NodeInterface> {
        reference
            .version_requirement()
            .matches(&self.version)
            .then_some(&self.interface)
    }

    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef> {
        (reference.name() == self.payload.name() && reference.version() == self.payload.version())
            .then_some(&self.payload)
    }
}

impl CompilationResolver for Resolver {
    fn resolve_node_version(&self, reference: &NodeReference) -> Option<&Version> {
        reference
            .version_requirement()
            .matches(&self.version)
            .then_some(&self.version)
    }
}

fn plan(policy: ExecutionPolicy) -> ExecutionPlan {
    let payload = TypeRef::new(
        "test.payload",
        1,
        SchemaDefinition::primitive(PrimitiveType::String),
    )
    .expect("test schema must be valid");
    let interface = NodeInterface::new(
        BTreeMap::from([(port("in"), InputPort::required(payload.clone()))]),
        BTreeMap::from([(port("out"), payload.clone())]),
    );
    let resolver = Resolver {
        payload,
        interface,
        version: Version::new(1, 0, 0),
    };
    let document = WorkflowDocument::new(
        WorkflowMetadata::new("resilience", Version::new(1, 0, 0)),
        WorkflowSpec::new(
            BTreeMap::from([(port("value"), type_reference())]),
            vec![NodeDefinition::new(
                NodeId::new("work").expect("test node id must be valid"),
                "test/work@^1"
                    .parse()
                    .expect("test reference must be valid"),
            )],
            vec![EdgeDefinition::new(
                "$inputs.value".parse().expect("source must be valid"),
                "work.in".parse().expect("target must be valid"),
            )],
            BTreeMap::from([(
                port("result"),
                Endpoint::node_port(
                    NodeId::new("work").expect("test node id must be valid"),
                    port("out"),
                ),
            )]),
        ),
    );

    compile_workflow(
        &document,
        &resolver,
        CompilationOptions::default().with_policy(policy),
    )
    .expect("resilience plan must compile")
}

fn scheduler() -> Scheduler {
    Scheduler::new(SchedulerConfig::new(
        NonZeroUsize::MIN,
        NonZeroUsize::MIN,
        1,
    ))
}

fn runtime() -> tokio::runtime::Runtime {
    Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .expect("test runtime must build")
}

fn test_run(id: &str, plan: &ExecutionPlan, options: RunOptions) -> Run {
    Run::new(
        RunId::new(id).expect("test run id must be valid"),
        plan,
        options,
    )
}

fn inputs(value: &str) -> WorkflowInputs {
    BTreeMap::from([(port("value"), text(value))])
}

fn type_reference() -> TypeReference {
    TypeReference::new("test.payload", 1).expect("test type reference must be valid")
}

fn port(value: &str) -> PortId {
    PortId::new(value).expect("test port must be valid")
}

fn text(value: &str) -> Payload {
    Payload::String(value.to_owned())
}

fn non_zero_u32(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("test attempt limit must be non-zero")
}
