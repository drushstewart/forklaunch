//! Process-local controls shared by all consumers of one worker runner.
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Running,
    Paused,
    Draining,
}

/// Clones address the same running process. Draining is irreversible; a restart
/// requires a new control. Pause stops future claims, while active handlers and
/// already submitted claims continue with lease renewal.
#[derive(Debug, Clone)]
pub struct WorkerControl(watch::Sender<WorkerState>);
impl Default for WorkerControl {
    fn default() -> Self {
        Self::new()
    }
}
impl WorkerControl {
    #[must_use]
    pub fn new() -> Self {
        Self(watch::channel(WorkerState::Running).0)
    }
    #[must_use]
    pub fn state(&self) -> WorkerState {
        *self.0.borrow()
    }
    /// Returns false after drain has started. Repeated pauses succeed.
    #[must_use]
    pub fn pause(&self) -> bool {
        self.set(WorkerState::Paused)
    }
    /// Returns false after drain has started. Repeated resumes succeed.
    #[must_use]
    pub fn resume(&self) -> bool {
        self.set(WorkerState::Running)
    }
    pub fn drain(&self) {
        self.set(WorkerState::Draining);
    }
    fn set(&self, next: WorkerState) -> bool {
        let mut accepted = false;
        self.0.send_if_modified(|state| {
            if *state == WorkerState::Draining {
                return false;
            }
            accepted = true;
            if *state == next {
                return false;
            }
            *state = next;
            true
        });
        accepted
    }
    /// Completes immediately if drain has already started.
    pub async fn draining(&self) {
        let mut receiver = self.0.subscribe();
        loop {
            if *receiver.borrow_and_update() == WorkerState::Draining {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<WorkerState> {
        self.0.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn control_retains_state_without_receivers_and_drain_is_sticky() {
        let control = WorkerControl::new();
        assert!(control.pause());
        let cloned = control.clone();
        assert_eq!(cloned.state(), WorkerState::Paused);
        assert!(cloned.resume());
        assert_eq!(control.state(), WorkerState::Running);
        let wait = control.draining();
        tokio::pin!(wait);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), &mut wait)
                .await
                .is_err()
        );
        cloned.drain();
        wait.await;
        assert!(!control.resume());
        assert!(!cloned.pause());
        control.drain();
        assert_eq!(control.state(), WorkerState::Draining);
        control.draining().await;
    }
}
