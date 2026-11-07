//! Owns the agent's long-running components and stops them together.
//!
//! Every component (collectors, buffer, uploader, heartbeat, local API…)
//! is spawned through [`Supervisor::spawn`] and must return promptly once its
//! [`CancellationToken`] is cancelled, after flushing whatever it holds.

use std::future::Future;
use std::time::Duration;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

pub struct Supervisor {
    token: CancellationToken,
    tasks: JoinSet<()>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// Every component stopped within the grace period.
    Clean,
    /// Some components ignored cancellation and were aborted.
    TimedOut { aborted: usize },
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

// `spawn`/`len` get their first non-test callers when components land (AG-2+).
#[allow(dead_code)]
impl Supervisor {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: JoinSet::new(),
        }
    }

    /// Spawns a component. `start` receives the token it must watch.
    /// Errors are logged with the component name; they do not stop the agent.
    pub fn spawn<F, Fut>(&mut self, name: &'static str, start: F)
    where
        F: FnOnce(CancellationToken) -> Fut,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let fut = start(self.token.child_token());
        self.tasks.spawn(async move {
            match fut.await {
                Ok(()) => debug!(component = name, "component stopped"),
                // Component errors must not carry event metadata (see logging.rs).
                Err(err) => {
                    error!(component = name, error = %format!("{err:#}"), "component failed")
                }
            }
        });
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Cancels every component and waits up to `grace` for them to finish.
    pub async fn shutdown(mut self, grace: Duration) -> ShutdownOutcome {
        self.token.cancel();
        let tasks = &mut self.tasks;
        let joined = tokio::time::timeout(grace, async {
            while let Some(result) = tasks.join_next().await {
                if let Err(err) = result
                    && err.is_panic()
                {
                    error!("component panicked");
                }
            }
        })
        .await;

        match joined {
            Ok(()) => ShutdownOutcome::Clean,
            Err(_) => {
                let aborted = self.tasks.len();
                warn!(aborted, "components did not stop in time; aborting");
                self.tasks.shutdown().await;
                ShutdownOutcome::TimedOut { aborted }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[tokio::test]
    async fn cooperative_components_stop_cleanly() {
        let mut supervisor = Supervisor::new();
        let flushed = Arc::new(AtomicBool::new(false));
        let flag = flushed.clone();
        supervisor.spawn("cooperative", move |token| async move {
            token.cancelled().await;
            flag.store(true, Ordering::SeqCst); // simulate a final flush
            Ok(())
        });
        assert_eq!(supervisor.len(), 1);

        let outcome = supervisor.shutdown(Duration::from_secs(5)).await;
        assert_eq!(outcome, ShutdownOutcome::Clean);
        assert!(flushed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failing_and_panicking_components_do_not_block_shutdown() {
        let mut supervisor = Supervisor::new();
        supervisor.spawn("fails", |_token| async { anyhow::bail!("boom") });
        supervisor.spawn("panics", |_token| async { panic!("boom") });
        let outcome = supervisor.shutdown(Duration::from_secs(5)).await;
        assert_eq!(outcome, ShutdownOutcome::Clean);
    }

    #[tokio::test]
    async fn stuck_components_are_aborted_after_grace() {
        let mut supervisor = Supervisor::new();
        supervisor.spawn("stuck", |_token| async {
            std::future::pending::<()>().await;
            Ok(())
        });
        let outcome = supervisor.shutdown(Duration::from_millis(50)).await;
        assert_eq!(outcome, ShutdownOutcome::TimedOut { aborted: 1 });
    }
}
