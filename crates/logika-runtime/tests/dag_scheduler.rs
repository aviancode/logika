//! Concurrent DAG scheduler integration tests.

#![allow(clippy::expect_used)]

use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use logika_core::{NodeId, Payload, PortId, PrimitiveType, RunId, SchemaDefinition, TypeRef};
use logika_runtime::{
    ExecutionError, NodeExecution, NodeExecutor, NodeFuture, NodeOutputs, Run, RunOptions,
    RunStatus, Scheduler, SchedulerConfig, WorkflowInputs,
};
use logika_workflow::{
    CompilationOptions, CompilationResolver, EdgeDefinition, Endpoint, ExecutionPlan, InputPort,
    NodeDefinition, NodeInterface, NodeReference, TypeReference, ValidationResolver,
    WorkflowDocument, WorkflowMetadata, WorkflowSpec, compile_workflow,
};
use semver::Version;
use tokio::runtime::Builder;

#[test]
fn executes_fan_out_concurrently_and_fan_in_after_both_branches() {
    runtime().block_on(async {
        let plan = fan_out_plan();
        let mut run = test_run("fan-out", &plan);
        let probe = Arc::new(ProbeExecutor::new(Duration::from_millis(40)));
        let executor: Arc<dyn NodeExecutor> = probe.clone();
        let scheduler = Scheduler::new(config(8, 4, 8));

        let result = scheduler
            .execute(&plan, &mut run, workflow_inputs("seed"), executor)
            .await
            .expect("valid fan-out plan must execute");

        assert_eq!(run.status(), RunStatus::Succeeded);
        assert_eq!(result.outputs().get(&port("result")), Some(&text("L:R")));
        assert!(probe.max_active() >= 2, "fan-out branches ran sequentially");
        assert_eq!(probe.join_inputs(), 2);
    });
}

#[test]
fn enforces_per_workflow_concurrency() {
    runtime().block_on(async {
        let plan = parallel_plan(6);
        let mut run = test_run("workflow-limit", &plan);
        let probe = Arc::new(ProbeExecutor::new(Duration::from_millis(25)));
        let executor: Arc<dyn NodeExecutor> = probe.clone();
        let scheduler = Scheduler::new(config(8, 2, 8));

        scheduler
            .execute(&plan, &mut run, workflow_inputs("value"), executor)
            .await
            .expect("parallel plan must execute within its limit");

        assert_eq!(probe.max_active(), 2);
        assert_eq!(run.status(), RunStatus::Succeeded);
    });
}

#[test]
fn shares_the_global_limit_across_workflow_runs() {
    runtime().block_on(async {
        let plan = Arc::new(parallel_plan(4));
        let scheduler = Scheduler::new(config(2, 4, 8));
        let probe = Arc::new(ProbeExecutor::new(Duration::from_millis(35)));

        let first = {
            let plan = Arc::clone(&plan);
            let scheduler = scheduler.clone();
            let executor: Arc<dyn NodeExecutor> = probe.clone();
            tokio::spawn(async move {
                let mut run = test_run("global-a", &plan);
                scheduler
                    .execute(&plan, &mut run, workflow_inputs("a"), executor)
                    .await
                    .map(|_| run.status())
            })
        };
        let second = {
            let plan = Arc::clone(&plan);
            let scheduler = scheduler.clone();
            let executor: Arc<dyn NodeExecutor> = probe.clone();
            tokio::spawn(async move {
                let mut run = test_run("global-b", &plan);
                scheduler
                    .execute(&plan, &mut run, workflow_inputs("b"), executor)
                    .await
                    .map(|_| run.status())
            })
        };

        let first = first.await.expect("first run task must not panic");
        let second = second.await.expect("second run task must not panic");
        assert_eq!(first.expect("first run must succeed"), RunStatus::Succeeded);
        assert_eq!(
            second.expect("second run must succeed"),
            RunStatus::Succeeded
        );
        assert_eq!(probe.max_active(), 2);
    });
}

#[test]
fn rejects_ready_work_when_the_bounded_queue_is_full() {
    runtime().block_on(async {
        let plan = parallel_plan(3);
        let mut run = test_run("queue-full", &plan);
        let executor: Arc<dyn NodeExecutor> =
            Arc::new(ProbeExecutor::new(Duration::from_millis(100)));
        let scheduler = Scheduler::new(config(1, 1, 1));

        let error = scheduler
            .execute(&plan, &mut run, workflow_inputs("value"), executor)
            .await
            .expect_err("two waiting roots must exceed capacity one");

        assert!(matches!(error, ExecutionError::QueueFull { capacity: 1 }));
        assert_eq!(run.status(), RunStatus::Failed);
    });
}

#[derive(Default)]
struct Resolver {
    nodes: BTreeMap<String, (Version, NodeInterface)>,
    payload: Option<TypeRef>,
}

impl ValidationResolver for Resolver {
    fn resolve_node(&self, reference: &NodeReference) -> Option<&NodeInterface> {
        let (version, interface) = self.nodes.get(reference.plugin().as_str())?;
        reference
            .version_requirement()
            .matches(version)
            .then_some(interface)
    }

    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef> {
        self.payload.as_ref().filter(|payload| {
            payload.name() == reference.name() && payload.version() == reference.version()
        })
    }
}

impl CompilationResolver for Resolver {
    fn resolve_node_version(&self, reference: &NodeReference) -> Option<&Version> {
        let (version, _) = self.nodes.get(reference.plugin().as_str())?;
        reference
            .version_requirement()
            .matches(version)
            .then_some(version)
    }
}

struct ProbeExecutor {
    delay: Duration,
    state: Arc<ProbeState>,
}

#[derive(Default)]
struct ProbeState {
    active: AtomicUsize,
    max_active: AtomicUsize,
    join_inputs: AtomicUsize,
}

impl ProbeExecutor {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            state: Arc::new(ProbeState::default()),
        }
    }

    fn max_active(&self) -> usize {
        self.state.max_active.load(Ordering::SeqCst)
    }

    fn join_inputs(&self) -> usize {
        self.state.join_inputs.load(Ordering::SeqCst)
    }
}

impl NodeExecutor for ProbeExecutor {
    fn execute(&self, execution: NodeExecution) -> NodeFuture {
        let delay = self.delay;
        let id = execution.node().id().as_str().to_owned();
        let inputs = execution.inputs().clone();
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
            state.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(delay).await;

            let output = match id.as_str() {
                "left" => text("L"),
                "right" => text("R"),
                "join" => {
                    let left = input_text(&inputs, "left");
                    let right = input_text(&inputs, "right");
                    state.join_inputs.store(
                        inputs.values().map(Vec::len).sum::<usize>(),
                        Ordering::SeqCst,
                    );
                    text(&format!("{left}:{right}"))
                }
                _ => inputs
                    .get(&port("in"))
                    .and_then(|values| values.first())
                    .cloned()
                    .expect("pass node must receive one input"),
            };
            state.active.fetch_sub(1, Ordering::SeqCst);
            Ok(NodeOutputs::from([(port("out"), output)]))
        })
    }
}

fn input_text<'a>(inputs: &'a BTreeMap<PortId, Vec<Payload>>, name: &str) -> &'a str {
    let value = inputs
        .get(&port(name))
        .and_then(|values| values.first())
        .expect("join input must be present");
    let Payload::String(value) = value else {
        panic!("join input must be text");
    };
    value
}

fn runtime() -> tokio::runtime::Runtime {
    Builder::new_multi_thread()
        .worker_threads(4)
        .enable_time()
        .build()
        .expect("test Tokio runtime must build")
}

fn config(global: usize, workflow: usize, queue: usize) -> SchedulerConfig {
    SchedulerConfig::new(non_zero(global), non_zero(workflow), queue)
}

fn non_zero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test limit must be non-zero")
}

fn workflow_inputs(value: &str) -> WorkflowInputs {
    BTreeMap::from([(port("value"), text(value))])
}

fn test_run(id: &str, plan: &ExecutionPlan) -> Run {
    Run::new(
        RunId::new(id).expect("test run id must be valid"),
        plan,
        RunOptions::default(),
    )
}

fn fan_out_plan() -> ExecutionPlan {
    let payload = payload_type();
    let pass = pass_interface(&payload);
    let join = NodeInterface::new(
        BTreeMap::from([
            (port("left"), InputPort::required(payload.clone())),
            (port("right"), InputPort::required(payload.clone())),
        ]),
        BTreeMap::from([(port("out"), payload.clone())]),
    );
    let resolver = Resolver {
        nodes: BTreeMap::from([
            ("test/pass".to_owned(), (Version::new(1, 0, 0), pass)),
            ("test/join".to_owned(), (Version::new(1, 0, 0), join)),
        ]),
        payload: Some(payload),
    };
    let document = WorkflowDocument::new(
        WorkflowMetadata::new("fan-out", Version::new(1, 0, 0)),
        WorkflowSpec::new(
            BTreeMap::from([(port("value"), type_reference())]),
            vec![
                node("source", "test/pass@^1"),
                node("left", "test/pass@^1"),
                node("right", "test/pass@^1"),
                node("join", "test/join@^1"),
            ],
            vec![
                edge("$inputs.value", "source.in"),
                edge("source.out", "left.in"),
                edge("source.out", "right.in"),
                edge("left.out", "join.left"),
                edge("right.out", "join.right"),
            ],
            BTreeMap::from([(port("result"), endpoint("join.out"))]),
        ),
    );
    compile_workflow(&document, &resolver, CompilationOptions::default())
        .expect("fan-out workflow must compile")
}

fn parallel_plan(count: usize) -> ExecutionPlan {
    let payload = payload_type();
    let resolver = Resolver {
        nodes: BTreeMap::from([(
            "test/pass".to_owned(),
            (Version::new(1, 0, 0), pass_interface(&payload)),
        )]),
        payload: Some(payload),
    };
    let nodes = (0..count)
        .map(|index| node(&format!("node-{index}"), "test/pass@^1"))
        .collect::<Vec<_>>();
    let edges = (0..count)
        .map(|index| edge("$inputs.value", &format!("node-{index}.in")))
        .collect::<Vec<_>>();
    let outputs = BTreeMap::from([(port("result"), endpoint("node-0.out"))]);
    let document = WorkflowDocument::new(
        WorkflowMetadata::new("parallel", Version::new(1, 0, 0)),
        WorkflowSpec::new(
            BTreeMap::from([(port("value"), type_reference())]),
            nodes,
            edges,
            outputs,
        ),
    );
    compile_workflow(&document, &resolver, CompilationOptions::default())
        .expect("parallel workflow must compile")
}

fn payload_type() -> TypeRef {
    TypeRef::new(
        "test.payload",
        1,
        SchemaDefinition::primitive(PrimitiveType::String),
    )
    .expect("test payload schema must be valid")
}

fn pass_interface(payload: &TypeRef) -> NodeInterface {
    NodeInterface::new(
        BTreeMap::from([(port("in"), InputPort::required(payload.clone()))]),
        BTreeMap::from([(port("out"), payload.clone())]),
    )
}

fn node(id: &str, uses: &str) -> NodeDefinition {
    NodeDefinition::new(
        NodeId::new(id).expect("test node id must be valid"),
        uses.parse().expect("test node reference must be valid"),
    )
}

fn edge(from: &str, to: &str) -> EdgeDefinition {
    EdgeDefinition::new(endpoint(from), endpoint(to))
}

fn endpoint(value: &str) -> Endpoint {
    value.parse().expect("test endpoint must be valid")
}

fn port(value: &str) -> PortId {
    PortId::new(value).expect("test port must be valid")
}

fn type_reference() -> TypeReference {
    TypeReference::new("test.payload", 1).expect("test type reference must be valid")
}

fn text(value: &str) -> Payload {
    Payload::String(value.to_owned())
}
