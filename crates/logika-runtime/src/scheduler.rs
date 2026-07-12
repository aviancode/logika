use std::{
    collections::{BTreeMap, VecDeque},
    error, fmt,
    future::Future,
    num::{NonZeroU32, NonZeroUsize},
    pin::Pin,
    sync::Arc,
};

use logika_core::{Error as NodeError, NodeId, Payload, PortId};
use logika_workflow::{Endpoint, ExecutionPlan, PlanNode};
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{NodeContext, Run, RunOutcome, RunStatus, RunTransitionError};

/// Payloads supplied to the declared workflow input ports.
pub type WorkflowInputs = BTreeMap<PortId, Payload>;

/// Payloads returned from the declared workflow output ports.
pub type WorkflowOutputs = BTreeMap<PortId, Payload>;

/// Payloads grouped by node input port in deterministic edge order.
///
/// A single-connection port contains one value. A `Many` port contains one
/// value for each incoming edge.
pub type NodeInputs = BTreeMap<PortId, Vec<Payload>>;

/// Payloads produced by one node invocation, keyed by output port.
pub type NodeOutputs = BTreeMap<PortId, Payload>;

/// Owned asynchronous result returned by a [`NodeExecutor`].
pub type NodeFuture =
    Pin<Box<dyn Future<Output = Result<NodeOutputs, NodeError>> + Send + 'static>>;

/// Object-safe host hook that executes one resolved node invocation.
///
/// The returned future owns its request so the scheduler can run independent
/// nodes concurrently. Retry and timeout behavior intentionally remain outside
/// this release stage.
pub trait NodeExecutor: Send + Sync + 'static {
    /// Executes one node attempt and returns all declared output payloads.
    fn execute(&self, execution: NodeExecution) -> NodeFuture;
}

/// Owned input passed to a [`NodeExecutor`].
#[derive(Debug)]
pub struct NodeExecution {
    node: PlanNode,
    inputs: NodeInputs,
    context: NodeContext,
}

impl NodeExecution {
    fn new(node: PlanNode, inputs: NodeInputs, context: NodeContext) -> Self {
        Self {
            node,
            inputs,
            context,
        }
    }

    /// Returns the immutable resolved plan node.
    #[must_use]
    pub const fn node(&self) -> &PlanNode {
        &self.node
    }

    /// Returns payloads grouped by input port.
    #[must_use]
    pub const fn inputs(&self) -> &NodeInputs {
        &self.inputs
    }

    /// Returns the context for this node attempt.
    #[must_use]
    pub const fn context(&self) -> &NodeContext {
        &self.context
    }
}

/// Limits applied by a [`Scheduler`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerConfig {
    global_concurrency: NonZeroUsize,
    workflow_concurrency: NonZeroUsize,
    queue_capacity: usize,
}

impl SchedulerConfig {
    /// Creates explicit global, per-workflow, and waiting-queue limits.
    #[must_use]
    pub const fn new(
        global_concurrency: NonZeroUsize,
        workflow_concurrency: NonZeroUsize,
        queue_capacity: usize,
    ) -> Self {
        Self {
            global_concurrency,
            workflow_concurrency,
            queue_capacity,
        }
    }

    /// Returns the maximum node invocations running across all workflows.
    #[must_use]
    pub const fn global_concurrency(self) -> NonZeroUsize {
        self.global_concurrency
    }

    /// Returns the maximum node invocations admitted by one workflow run.
    #[must_use]
    pub const fn workflow_concurrency(self) -> NonZeroUsize {
        self.workflow_concurrency
    }

    /// Returns the waiting-node limit for a run and shared scheduler admission.
    #[must_use]
    pub const fn queue_capacity(self) -> usize {
        self.queue_capacity
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(64).unwrap_or(NonZeroUsize::MIN),
            NonZeroUsize::new(16).unwrap_or(NonZeroUsize::MIN),
            1_024,
        )
    }
}

/// Concurrent DAG scheduler with shared global admission controls.
///
/// Clones share the global semaphore, so independently started workflow runs
/// still obey one process-wide node concurrency limit.
#[derive(Clone, Debug)]
pub struct Scheduler {
    config: SchedulerConfig,
    global: Arc<Semaphore>,
    admission: Arc<Semaphore>,
}

impl Scheduler {
    /// Creates a scheduler with explicit resource limits.
    #[must_use]
    pub fn new(config: SchedulerConfig) -> Self {
        let global = config.global_concurrency.get();
        Self {
            config,
            global: Arc::new(Semaphore::new(global)),
            admission: Arc::new(Semaphore::new(global.saturating_add(config.queue_capacity))),
        }
    }

    /// Returns the scheduler limits.
    #[must_use]
    pub const fn config(&self) -> SchedulerConfig {
        self.config
    }

    /// Executes one validated plan and updates its run lifecycle.
    ///
    /// Nodes become eligible only after every direct dependency succeeds.
    /// Independent nodes are dispatched concurrently, while workflow and
    /// process-wide semaphores bound active work. Ready nodes that cannot be
    /// admitted within the configured queue capacity fail explicitly.
    pub async fn execute(
        &self,
        plan: &ExecutionPlan,
        run: &mut Run,
        inputs: WorkflowInputs,
        executor: Arc<dyn NodeExecutor>,
    ) -> Result<ExecutionResult, ExecutionError> {
        self.validate_run(plan, run)?;
        if let Err(error) = validate_workflow_inputs(plan, &inputs) {
            finish_failed(run)?;
            return Err(error);
        }

        let nodes = plan
            .nodes()
            .iter()
            .cloned()
            .map(|node| (node.id().clone(), node))
            .collect::<BTreeMap<_, _>>();
        let mut remaining = nodes
            .values()
            .map(|node| (node.id().clone(), node.dependencies().len()))
            .collect::<BTreeMap<_, _>>();
        let mut dependents = nodes
            .keys()
            .cloned()
            .map(|node| (node, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        for node in nodes.values() {
            for dependency in node.dependencies() {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .push(node.id().clone());
            }
        }

        let mut ready = remaining
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(node, _)| node.clone())
            .collect::<VecDeque<_>>();
        let workflow = Arc::new(Semaphore::new(self.config.workflow_concurrency.get()));
        let mut tasks = JoinSet::new();
        let mut node_outputs = BTreeMap::<NodeId, NodeOutputs>::new();
        let mut completed = 0usize;

        while completed < nodes.len() {
            while tasks.len() < self.config.workflow_concurrency.get() {
                let Some(node_id) = ready.pop_front() else {
                    break;
                };
                let Some(node) = nodes.get(&node_id).cloned() else {
                    return fail(
                        run,
                        &mut tasks,
                        ExecutionError::InvalidPlan(format!("plan node {node_id} disappeared")),
                    )
                    .await;
                };
                let node_inputs = match collect_node_inputs(plan, &node_id, &inputs, &node_outputs)
                {
                    Ok(inputs) => inputs,
                    Err(error) => return fail(run, &mut tasks, error).await,
                };
                let admission = match Arc::clone(&self.admission).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        return fail(
                            run,
                            &mut tasks,
                            ExecutionError::QueueFull {
                                capacity: self.config.queue_capacity,
                            },
                        )
                        .await;
                    }
                };
                let global = Arc::clone(&self.global);
                let workflow = Arc::clone(&workflow);
                let executor = Arc::clone(&executor);
                let context = run.node_context(node_id.clone(), NonZeroU32::MIN);
                tasks.spawn(async move {
                    let _admission = admission;
                    let _workflow = workflow
                        .acquire_owned()
                        .await
                        .map_err(|_| ExecutionError::SchedulerClosed)?;
                    let _global = global
                        .acquire_owned()
                        .await
                        .map_err(|_| ExecutionError::SchedulerClosed)?;
                    let result = executor
                        .execute(NodeExecution::new(node, node_inputs, context))
                        .await
                        .map_err(|source| ExecutionError::NodeFailed {
                            node: node_id.clone(),
                            source,
                        });
                    Ok::<_, ExecutionError>((node_id, result?))
                });
            }

            if ready.len() > self.config.queue_capacity {
                return fail(
                    run,
                    &mut tasks,
                    ExecutionError::QueueFull {
                        capacity: self.config.queue_capacity,
                    },
                )
                .await;
            }

            let Some(joined) = tasks.join_next().await else {
                return fail(
                    run,
                    &mut tasks,
                    ExecutionError::InvalidPlan(
                        "DAG execution stalled before all nodes completed".to_owned(),
                    ),
                )
                .await;
            };
            let (node_id, outputs) = match joined {
                Ok(Ok(completed)) => completed,
                Ok(Err(error)) => return fail(run, &mut tasks, error).await,
                Err(source) => {
                    return fail(run, &mut tasks, ExecutionError::TaskFailed(source)).await;
                }
            };
            if let Err(error) = validate_node_outputs(&nodes[&node_id], &outputs) {
                return fail(run, &mut tasks, error).await;
            }
            node_outputs.insert(node_id.clone(), outputs);
            completed += 1;

            for dependent in &dependents[&node_id] {
                let count = remaining.get_mut(dependent).ok_or_else(|| {
                    ExecutionError::InvalidPlan(format!(
                        "dependency counter for node {dependent} is missing"
                    ))
                });
                let count = match count {
                    Ok(count) => count,
                    Err(error) => return fail(run, &mut tasks, error).await,
                };
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.push_back(dependent.clone());
                }
            }
        }

        let outputs = match collect_workflow_outputs(plan, &inputs, &node_outputs) {
            Ok(outputs) => outputs,
            Err(error) => return fail(run, &mut tasks, error).await,
        };
        run.finish(RunOutcome::Succeeded)?;
        Ok(ExecutionResult { outputs })
    }

    fn validate_run(&self, plan: &ExecutionPlan, run: &Run) -> Result<(), ExecutionError> {
        if run.status() != RunStatus::Running {
            return Err(ExecutionError::RunNotRunning(run.status()));
        }
        if run.metadata().plan_hash() != plan.plan_hash() {
            return Err(ExecutionError::PlanMismatch);
        }
        Ok(())
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(SchedulerConfig::default())
    }
}

/// Successful workflow execution result.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionResult {
    outputs: WorkflowOutputs,
}

impl ExecutionResult {
    /// Returns the named workflow outputs.
    #[must_use]
    pub const fn outputs(&self) -> &WorkflowOutputs {
        &self.outputs
    }

    /// Consumes the result and returns its named workflow outputs.
    #[must_use]
    pub fn into_outputs(self) -> WorkflowOutputs {
        self.outputs
    }
}

/// Failure produced while scheduling or executing a DAG.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExecutionError {
    /// The lifecycle record does not belong to the supplied plan.
    PlanMismatch,
    /// The lifecycle record was already terminal.
    RunNotRunning(RunStatus),
    /// A required workflow input was absent or invalid.
    InvalidWorkflowInput {
        /// Workflow input port affected by the failure.
        port: PortId,
        /// Safe validation diagnostic.
        message: String,
    },
    /// A node returned missing, unknown, or invalid output data.
    InvalidNodeOutput {
        /// Node that returned the invalid output.
        node: NodeId,
        /// Output port affected by the failure.
        port: PortId,
        /// Safe validation diagnostic.
        message: String,
    },
    /// A ready node could not be admitted without exceeding the queue limit.
    QueueFull {
        /// Configured number of waiting nodes.
        capacity: usize,
    },
    /// A node executor returned a classified error.
    NodeFailed {
        /// Workflow-local node that failed.
        node: NodeId,
        /// Classified executor failure.
        source: NodeError,
    },
    /// A scheduler worker panicked or was cancelled unexpectedly.
    TaskFailed(tokio::task::JoinError),
    /// A semaphore was closed while execution was active.
    SchedulerClosed,
    /// The supplied plan violated an invariant required by DAG execution.
    InvalidPlan(String),
    /// The run lifecycle rejected the terminal transition.
    RunTransition(RunTransitionError),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanMismatch => formatter.write_str("run and execution plan hashes do not match"),
            Self::RunNotRunning(status) => write!(formatter, "run is already {status}"),
            Self::InvalidWorkflowInput { port, message } => {
                write!(formatter, "workflow input {port} is invalid: {message}")
            }
            Self::InvalidNodeOutput {
                node,
                port,
                message,
            } => write!(formatter, "output {node}.{port} is invalid: {message}"),
            Self::QueueFull { capacity } => {
                write!(
                    formatter,
                    "scheduler queue capacity {capacity} was exceeded"
                )
            }
            Self::NodeFailed { node, source } => write!(formatter, "node {node} failed: {source}"),
            Self::TaskFailed(source) => write!(formatter, "scheduler task failed: {source}"),
            Self::SchedulerClosed => formatter.write_str("scheduler semaphore was closed"),
            Self::InvalidPlan(message) => write!(formatter, "invalid execution plan: {message}"),
            Self::RunTransition(source) => source.fmt(formatter),
        }
    }
}

impl error::Error for ExecutionError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::NodeFailed { source, .. } => Some(source),
            Self::TaskFailed(source) => Some(source),
            Self::RunTransition(source) => Some(source),
            _ => None,
        }
    }
}

impl From<RunTransitionError> for ExecutionError {
    fn from(source: RunTransitionError) -> Self {
        Self::RunTransition(source)
    }
}

fn validate_workflow_inputs(
    plan: &ExecutionPlan,
    inputs: &WorkflowInputs,
) -> Result<(), ExecutionError> {
    for (port, type_ref) in plan.inputs() {
        let payload = inputs
            .get(port)
            .ok_or_else(|| ExecutionError::InvalidWorkflowInput {
                port: port.clone(),
                message: "required input is missing".to_owned(),
            })?;
        type_ref.validate_payload(payload).map_err(|source| {
            ExecutionError::InvalidWorkflowInput {
                port: port.clone(),
                message: source.to_string(),
            }
        })?;
    }
    if let Some(port) = inputs
        .keys()
        .find(|port| !plan.inputs().contains_key(*port))
    {
        return Err(ExecutionError::InvalidWorkflowInput {
            port: port.clone(),
            message: "input is not declared by the workflow".to_owned(),
        });
    }
    Ok(())
}

fn collect_node_inputs(
    plan: &ExecutionPlan,
    node_id: &NodeId,
    workflow_inputs: &WorkflowInputs,
    node_outputs: &BTreeMap<NodeId, NodeOutputs>,
) -> Result<NodeInputs, ExecutionError> {
    let mut inputs = NodeInputs::new();
    for edge in plan.edges() {
        let Endpoint::NodePort { node, port } = edge.to() else {
            return Err(ExecutionError::InvalidPlan(format!(
                "edge target {} is not a node input",
                edge.to()
            )));
        };
        if node != node_id {
            continue;
        }
        let payload = resolve_endpoint(edge.from(), workflow_inputs, node_outputs)?;
        inputs.entry(port.clone()).or_default().push(payload);
    }
    Ok(inputs)
}

fn validate_node_outputs(node: &PlanNode, outputs: &NodeOutputs) -> Result<(), ExecutionError> {
    for (port, type_ref) in node.interface().outputs() {
        let payload = outputs
            .get(port)
            .ok_or_else(|| ExecutionError::InvalidNodeOutput {
                node: node.id().clone(),
                port: port.clone(),
                message: "declared output is missing".to_owned(),
            })?;
        type_ref
            .validate_payload(payload)
            .map_err(|source| ExecutionError::InvalidNodeOutput {
                node: node.id().clone(),
                port: port.clone(),
                message: source.to_string(),
            })?;
    }
    if let Some(port) = outputs
        .keys()
        .find(|port| !node.interface().outputs().contains_key(*port))
    {
        return Err(ExecutionError::InvalidNodeOutput {
            node: node.id().clone(),
            port: port.clone(),
            message: "output is not declared by the node interface".to_owned(),
        });
    }
    Ok(())
}

fn collect_workflow_outputs(
    plan: &ExecutionPlan,
    workflow_inputs: &WorkflowInputs,
    node_outputs: &BTreeMap<NodeId, NodeOutputs>,
) -> Result<WorkflowOutputs, ExecutionError> {
    plan.outputs()
        .iter()
        .map(|(port, output)| {
            resolve_endpoint(output.source(), workflow_inputs, node_outputs)
                .map(|payload| (port.clone(), payload))
        })
        .collect()
}

fn resolve_endpoint(
    endpoint: &Endpoint,
    workflow_inputs: &WorkflowInputs,
    node_outputs: &BTreeMap<NodeId, NodeOutputs>,
) -> Result<Payload, ExecutionError> {
    match endpoint {
        Endpoint::WorkflowInput(port) => workflow_inputs
            .get(port)
            .cloned()
            .ok_or_else(|| ExecutionError::InvalidPlan(format!("input {port} is unavailable"))),
        Endpoint::NodePort { node, port } => node_outputs
            .get(node)
            .and_then(|outputs| outputs.get(port))
            .cloned()
            .ok_or_else(|| {
                ExecutionError::InvalidPlan(format!("node output {node}.{port} is unavailable"))
            }),
        _ => Err(ExecutionError::InvalidPlan(format!(
            "unsupported endpoint {endpoint}"
        ))),
    }
}

async fn fail<T>(
    run: &mut Run,
    tasks: &mut JoinSet<Result<(NodeId, NodeOutputs), ExecutionError>>,
    error: ExecutionError,
) -> Result<T, ExecutionError> {
    tasks.shutdown().await;
    finish_failed(run)?;
    Err(error)
}

fn finish_failed(run: &mut Run) -> Result<(), ExecutionError> {
    run.finish(RunOutcome::Failed).map_err(ExecutionError::from)
}
