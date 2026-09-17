//! Orderly shutdown (#102).
//!
//! The server used to be `SIGKILL`ed on every deployment, and a `SIGKILL`
//! unwinds nothing: in-flight responses are severed mid-write and every spawned
//! task dies where it stands. Stopping the process politely is the first half
//! of fixing that, and it lives in `main`. This module is the second half --
//! making the *work* stop politely too -- because a graceful HTTP shutdown that
//! still drops a half-delivered webhook has only moved the problem.
//!
//! Two mechanisms, because there are two kinds of background work and they want
//! opposite treatment:
//!
//! * **Daemons** -- the pages updater, the MQTT service, the bypass sweep, the
//!   schedule tickers. These loop forever by design. Waiting for one to finish
//!   would hang shutdown, so they are *cancelled*: [`Shutdown::cancelled`]
//!   resolves and the loop returns. What that buys is that they stop **between**
//!   iterations rather than in the middle of one -- the pages updater is not cut
//!   part-way through a `git` subprocess.
//!
//! * **Detached work** -- a webhook delivery already in flight. These finish in
//!   bounded time and losing one loses information, so they are *awaited*:
//!   spawn them with [`Shutdown::spawn`] and [`Shutdown::finish`] waits for
//!   them.
//!
//! Getting these the wrong way round is the failure worth guarding against. A
//! daemon spawned onto the tracker never completes, so `finish` would block
//! until its deadline on every single shutdown. The deadline is therefore not
//! optional, and it is why `finish` reports whether it drained or timed out
//! rather than returning `()`.
//!
//! What this deliberately does not do: persist work across a restart. A
//! delivery still in flight when the deadline expires is still lost. The claim
//! is bounded -- work in progress gets a fair chance to finish, and anything
//! that does not is reported rather than silently dropped.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// How long detached work gets to finish once shutdown starts.
///
/// Comfortably inside the container's ten-second stop timeout: overrunning that
/// would earn the `SIGKILL` this exists to avoid, which would be a comic way to
/// fail.
pub const DRAIN_DEADLINE: Duration = Duration::from_secs(5);

/// The handle background work uses to find out that the server is stopping.
#[derive(Clone, Debug)]
pub struct Shutdown {
    token: CancellationToken,
    tracker: TaskTracker,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tracker: TaskTracker::new(),
        }
    }

    /// Resolves when the server has been asked to stop.
    ///
    /// Daemon loops select on this. Cloned rather than borrowed so a loop can
    /// own it for the life of the task.
    pub async fn cancelled(&self) {
        self.token.cancelled().await
    }

    /// A token for callers that need to hand cancellation somewhere else.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Spawn work that shutdown should **wait for**.
    ///
    /// Only for tasks that finish on their own. A loop spawned here will still
    /// be running when `finish` is called and will hold it open until the
    /// deadline expires.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.tracker.spawn(future)
    }

    /// Ask everything to stop, then wait for the tracked work to finish.
    ///
    /// Returns `true` if it drained within [`DRAIN_DEADLINE`], `false` if the
    /// deadline expired with work still running. The caller logs the
    /// difference: "we stopped cleanly" and "we gave up waiting and some
    /// deliveries were lost" are different events and should not read the same
    /// in a log.
    pub async fn finish(&self) -> bool {
        self.finish_within(DRAIN_DEADLINE).await
    }

    /// [`Shutdown::finish`] with an explicit deadline.
    ///
    /// Exists so the deadline is reachable from a test. A timeout that only
    /// production can reach is a timeout nobody has watched fire, and the case
    /// it guards -- a daemon mistakenly spawned onto the tracker, which never
    /// completes -- is the one failure that would make every shutdown hang
    /// until the deadline.
    pub async fn finish_within(&self, deadline: Duration) -> bool {
        self.token.cancel();
        self.tracker.close();
        tokio::time::timeout(deadline, self.tracker.wait())
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn a_daemon_loop_stops_when_cancelled() {
        let shutdown = Shutdown::new();
        let stopped = Arc::new(AtomicBool::new(false));

        let (s, flag) = (shutdown.clone(), stopped.clone());
        let daemon = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = s.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
            }
            flag.store(true, Ordering::SeqCst);
        });

        assert!(shutdown.finish().await);
        daemon.await.unwrap();
        assert!(
            stopped.load(Ordering::SeqCst),
            "the loop never observed the cancellation"
        );
    }

    #[tokio::test]
    async fn tracked_work_is_waited_for() {
        let shutdown = Shutdown::new();
        let done = Arc::new(AtomicBool::new(false));

        let flag = done.clone();
        shutdown.spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, Ordering::SeqCst);
        });

        assert!(
            shutdown.finish().await,
            "should have drained inside the deadline"
        );
        assert!(
            done.load(Ordering::SeqCst),
            "finish() returned before the tracked work completed, so it is not \
             actually draining anything"
        );
    }

    /// The failure this module's documentation warns about, asserted rather
    /// than merely described: work that never finishes must make `finish`
    /// report a timeout, not hang forever.
    #[tokio::test]
    async fn work_that_never_finishes_times_out_rather_than_hanging() {
        let shutdown = Shutdown::new();
        shutdown.spawn(std::future::pending::<()>());

        assert!(
            !shutdown.finish_within(Duration::from_millis(50)).await,
            "a task that never completes must be reported as a failed drain"
        );
    }

    /// Cancellation has to be observable by work spawned after the token was
    /// created but before shutdown -- the ordinary case, and one that breaks if
    /// the token is ever replaced rather than shared.
    #[tokio::test]
    async fn cancellation_reaches_a_clone() {
        let shutdown = Shutdown::new();
        let clone = shutdown.clone();
        let waiter = tokio::spawn(async move { clone.cancelled().await });

        shutdown.finish().await;
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("the clone never saw the cancellation")
            .unwrap();
    }
}
