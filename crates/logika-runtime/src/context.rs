use std::{num::NonZeroU32, sync::Arc, time::Duration};

use logika_core::NodeId;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::{Run, RunMetadata};

/// Immutable execution context supplied to one node attempt.
///
/// Contexts are cheap to clone for asynchronous work. Every clone shares the
/// same run metadata and cancellation token while preserving the node and
/// attempt identity used to create it.
#[derive(Clone, Debug)]
pub struct NodeContext {
    run: Arc<RunMetadata>,
    node_id: NodeId,
    attempt: NonZeroU32,
    deadline: Option<Instant>,
    cancellation_token: CancellationToken,
}

impl NodeContext {
    pub(crate) fn from_run(run: &Run, node_id: NodeId, attempt: NonZeroU32) -> Self {
        Self {
            run: run.shared_metadata(),
            node_id,
            attempt,
            deadline: run.deadline(),
            cancellation_token: run.cancellation_token(),
        }
    }

    /// Returns immutable metadata for the parent workflow run.
    #[must_use]
    pub fn run(&self) -> &RunMetadata {
        &self.run
    }

    /// Returns the workflow-local node identifier.
    #[must_use]
    pub const fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns the one-based execution attempt number.
    #[must_use]
    pub const fn attempt(&self) -> NonZeroU32 {
        self.attempt
    }

    /// Returns the absolute deadline inherited from the run, if configured.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Returns the time remaining until the inherited run deadline.
    ///
    /// A configured deadline that has already elapsed yields zero.
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// Returns whether the inherited run deadline has elapsed.
    #[must_use]
    pub fn deadline_exceeded(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Returns a token connected to the parent run's cancellation context.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Returns whether cancellation has already been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Waits until cancellation is requested for the parent run.
    pub async fn cancelled(&self) {
        self.cancellation_token.cancelled().await;
    }
}
