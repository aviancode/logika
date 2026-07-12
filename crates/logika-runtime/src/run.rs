use std::{error, fmt, sync::Arc, time::SystemTime};

use logika_core::{NodeId, RunId};
use logika_workflow::{ExecutionPlan, PlanHash};
use semver::Version;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::NodeContext;

/// Host-provided controls and correlation metadata for a workflow run.
///
/// Cloning these options deliberately shares the cancellation token. This
/// lets an embedding application retain a token and cancel a run after it has
/// handed the options to the runtime.
#[derive(Clone, Debug)]
pub struct RunOptions {
    correlation_id: Option<String>,
    deadline: Option<Instant>,
    cancellation_token: CancellationToken,
}

impl RunOptions {
    /// Attaches a host-defined correlation identifier to the run.
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Sets the absolute Tokio deadline for the complete run.
    ///
    /// Deadline enforcement is performed by the runtime resilience layer.
    /// The run model carries the value so every node observes the same bound.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Uses a caller-owned token for cooperative run cancellation.
    ///
    /// The caller may retain a clone and invoke [`CancellationToken::cancel`]
    /// at any time.
    #[must_use]
    pub fn with_cancellation_token(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = cancellation_token;
        self
    }

    /// Returns the optional host-defined correlation identifier.
    #[must_use]
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Returns the absolute deadline for the complete run, if configured.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Returns a token connected to the run's cancellation context.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            correlation_id: None,
            deadline: None,
            cancellation_token: CancellationToken::new(),
        }
    }
}

/// Immutable identity and correlation metadata shared by a run and its nodes.
#[derive(Clone, Debug)]
pub struct RunMetadata {
    run_id: RunId,
    workflow_name: String,
    workflow_version: Version,
    plan_hash: PlanHash,
    started_at: SystemTime,
    correlation_id: String,
}

impl RunMetadata {
    /// Returns the host-assigned stable run identifier.
    #[must_use]
    pub const fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// Returns the workflow name captured by the compiled plan.
    #[must_use]
    pub fn workflow_name(&self) -> &str {
        &self.workflow_name
    }

    /// Returns the workflow version captured by the compiled plan.
    #[must_use]
    pub const fn workflow_version(&self) -> &Version {
        &self.workflow_version
    }

    /// Returns the immutable compiled plan identity used by this run.
    #[must_use]
    pub const fn plan_hash(&self) -> PlanHash {
        self.plan_hash
    }

    /// Returns the wall-clock time at which runtime execution began.
    #[must_use]
    pub const fn started_at(&self) -> SystemTime {
        self.started_at
    }

    /// Returns the host-defined correlation identifier, or the run ID fallback.
    #[must_use]
    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

/// Current lifecycle state of a workflow run.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum RunStatus {
    /// The runtime is executing or preparing to execute the plan.
    Running,
    /// Every required workflow output was produced successfully.
    Succeeded,
    /// Execution stopped because a node or runtime operation failed.
    Failed,
    /// Execution stopped after cooperative cancellation was requested.
    Cancelled,
    /// Execution exceeded its run deadline.
    TimedOut,
}

impl RunStatus {
    /// Returns whether no further lifecycle transition is valid.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        })
    }
}

/// Terminal outcome supplied when execution of a run ends.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum RunOutcome {
    /// Every required workflow output was produced successfully.
    Succeeded,
    /// Execution stopped because a node or runtime operation failed.
    Failed,
    /// Execution stopped after cooperative cancellation was requested.
    Cancelled,
    /// Execution exceeded its run deadline.
    TimedOut,
}

impl RunOutcome {
    const fn status(self) -> RunStatus {
        match self {
            Self::Succeeded => RunStatus::Succeeded,
            Self::Failed => RunStatus::Failed,
            Self::Cancelled => RunStatus::Cancelled,
            Self::TimedOut => RunStatus::TimedOut,
        }
    }
}

/// A single in-memory workflow run lifecycle record.
///
/// Identity fields are captured from the immutable execution plan when the
/// run starts and cannot be changed afterward. Only the lifecycle status and
/// completion timestamp transition, once, from running to a terminal state.
#[derive(Debug)]
pub struct Run {
    metadata: Arc<RunMetadata>,
    options: RunOptions,
    status: RunStatus,
    finished_at: Option<SystemTime>,
}

impl Run {
    /// Starts a lifecycle record for an immutable execution plan.
    #[must_use]
    pub fn new(run_id: RunId, plan: &ExecutionPlan, options: RunOptions) -> Self {
        Self::new_at(run_id, plan, options, SystemTime::now())
    }

    /// Starts a lifecycle record at a caller-supplied wall-clock time.
    ///
    /// This constructor supports deterministic host adapters and tests. The
    /// supplied time is captured as immutable metadata.
    #[must_use]
    pub fn new_at(
        run_id: RunId,
        plan: &ExecutionPlan,
        options: RunOptions,
        started_at: SystemTime,
    ) -> Self {
        let correlation_id = options
            .correlation_id
            .clone()
            .unwrap_or_else(|| run_id.as_str().to_owned());
        let metadata = RunMetadata {
            run_id,
            workflow_name: plan.workflow_name().to_owned(),
            workflow_version: plan.workflow_version().clone(),
            plan_hash: plan.plan_hash(),
            started_at,
            correlation_id,
        };

        Self {
            metadata: Arc::new(metadata),
            options,
            status: RunStatus::Running,
            finished_at: None,
        }
    }

    /// Returns the immutable metadata shared with node contexts.
    #[must_use]
    pub fn metadata(&self) -> &RunMetadata {
        &self.metadata
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub const fn status(&self) -> RunStatus {
        self.status
    }

    /// Returns the wall-clock completion time for a terminal run.
    #[must_use]
    pub const fn finished_at(&self) -> Option<SystemTime> {
        self.finished_at
    }

    /// Returns the absolute deadline for the complete run, if configured.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.options.deadline()
    }

    /// Returns a token connected to the run's cancellation context.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.options.cancellation_token()
    }

    /// Creates an execution context for one node attempt in this run.
    #[must_use]
    pub fn node_context(&self, node_id: NodeId, attempt: std::num::NonZeroU32) -> NodeContext {
        NodeContext::from_run(self, node_id, attempt)
    }

    /// Transitions a running lifecycle record to its terminal outcome.
    pub fn finish(&mut self, outcome: RunOutcome) -> Result<(), RunTransitionError> {
        self.finish_at(outcome, SystemTime::now())
    }

    /// Transitions a run at a caller-supplied wall-clock completion time.
    pub fn finish_at(
        &mut self,
        outcome: RunOutcome,
        finished_at: SystemTime,
    ) -> Result<(), RunTransitionError> {
        if self.status.is_terminal() {
            return Err(RunTransitionError {
                current: self.status,
                requested: outcome.status(),
            });
        }

        self.status = outcome.status();
        self.finished_at = Some(finished_at);
        Ok(())
    }

    pub(crate) fn shared_metadata(&self) -> Arc<RunMetadata> {
        Arc::clone(&self.metadata)
    }
}

/// Error returned when code tries to finish an already terminal run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunTransitionError {
    current: RunStatus,
    requested: RunStatus,
}

impl RunTransitionError {
    /// Returns the terminal state already held by the run.
    #[must_use]
    pub const fn current(&self) -> RunStatus {
        self.current
    }

    /// Returns the terminal state requested by the rejected transition.
    #[must_use]
    pub const fn requested(&self) -> RunStatus {
        self.requested
    }
}

impl fmt::Display for RunTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot transition run from {} to {}",
            self.current, self.requested
        )
    }
}

impl error::Error for RunTransitionError {}
