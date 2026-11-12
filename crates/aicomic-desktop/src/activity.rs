//! In-process writer lifetimes. Cancellation is not proof that a blocking worker
//! has finished; only the last owner of its project lease makes it quiescent.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct ProjectActivityTracker {
    counts: Arc<Mutex<HashMap<i64, usize>>>,
}

#[derive(Clone)]
pub struct ProjectActivityLease {
    owner: Arc<Owner>,
}

struct Owner {
    project: i64,
    counts: Arc<Mutex<HashMap<i64, usize>>>,
}

impl ProjectActivityTracker {
    pub fn enter(&self, project: i64) -> Result<ProjectActivityLease, String> {
        if project <= 0 {
            return Err("后台任务项目 ID 无效".into());
        }
        let mut counts = self
            .counts
            .lock()
            .map_err(|_| "后台写入状态锁不可用".to_owned())?;
        let count = counts.entry(project).or_default();
        *count = count.checked_add(1).ok_or("后台写入活动计数溢出")?;
        Ok(ProjectActivityLease {
            owner: Arc::new(Owner {
                project,
                counts: Arc::clone(&self.counts),
            }),
        })
    }

    pub fn is_active(&self, project: i64) -> bool {
        self.counts.lock().map_or(true, |counts| {
            counts.get(&project).copied().unwrap_or(0) > 0
        })
    }

    pub fn any_active(&self) -> bool {
        self.counts
            .lock()
            .map_or(true, |counts| counts.values().any(|count| *count > 0))
    }
}

impl ProjectActivityLease {
    pub fn project(&self) -> i64 {
        self.owner.project
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.counts.lock() {
            match counts
                .get_mut(&self.project)
                .and_then(|count| count.checked_sub(1))
            {
                Some(0) => {
                    counts.remove(&self.project);
                }
                Some(remaining) => {
                    counts.insert(self.project, remaining);
                }
                // A corrupted count must never make deletion appear safe.
                None => {
                    counts.insert(self.project, usize::MAX);
                }
            }
        }
    }
}

/// Keep a blocking writer's lease in the worker, not its cancellable awaiter.
pub async fn run_blocking<T, F>(
    activity: Option<ProjectActivityLease>,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let _activity = activity;
        operation()
    })
    .await
    .map_err(|error| error.to_string())
}

/// An owned async writer can hide its own non-cancellable blocking tasks. Keep
/// awaiting it in a supervised task even if the GUI stops awaiting the result.
pub async fn finish<T, F>(activity: ProjectActivityLease, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
{
    tokio::spawn(async move {
        let _activity = activity;
        operation.await
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_last_lease_owner_releases_a_project_without_affecting_other_projects() {
        let tracker = ProjectActivityTracker::default();
        let first = tracker.enter(1).unwrap();
        let worker = first.clone();
        let second = tracker.enter(2).unwrap();
        assert_eq!(worker.project(), 1);
        assert!(tracker.is_active(1));
        assert!(tracker.is_active(2));
        drop(first);
        assert!(tracker.is_active(1), "a clone still owns the actual worker");
        drop(worker);
        assert!(
            !tracker.is_active(1),
            "the last owner makes only its project quiescent"
        );
        assert!(tracker.is_active(2));
        drop(second);
        assert!(!tracker.any_active());
    }

    #[derive(Debug)]
    struct Finished(std::sync::mpsc::Sender<()>);
    impl Drop for Finished {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    #[test]
    fn aborted_blocking_awaiter_remains_active_until_the_real_worker_exits() {
        let tracker = ProjectActivityTracker::default();
        let lease = tracker.enter(1).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let task = runtime.spawn(run_blocking(Some(lease), move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Finished(finished_tx)
        }));
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        task.abort();
        assert!(runtime.block_on(task).unwrap_err().is_cancelled());
        let still_active = tracker.is_active(1);
        release_tx.send(()).unwrap();
        finished_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(
            still_active,
            "cancelling the awaiter must not release its live writer"
        );
        assert!(!tracker.is_active(1));
    }

    #[test]
    fn cancelled_async_awaiter_retains_lease_for_an_inner_non_cancellable_worker() {
        let tracker = ProjectActivityTracker::default();
        let lease = tracker.enter(1).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let task = runtime.spawn(finish(lease, async move {
            tokio::task::spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Finished(finished_tx)
            })
            .await
            .unwrap()
        }));
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        task.abort();
        assert!(runtime.block_on(task).unwrap_err().is_cancelled());
        let still_active = tracker.is_active(1);
        release_tx.send(()).unwrap();
        finished_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(
            still_active,
            "the memory Adapter's detached worker must still block deletion"
        );
        assert!(!tracker.is_active(1));
    }

    #[test]
    fn inconsistent_counts_fail_closed_without_panicking_or_accepting_more_work() {
        let tracker = ProjectActivityTracker::default();
        let lease = tracker.enter(1).unwrap();
        tracker.counts.lock().unwrap().clear();
        drop(lease);
        assert!(tracker.is_active(1));
        assert!(tracker.any_active());
        assert!(tracker.enter(1).is_err());
    }

    #[test]
    fn poisoned_activity_state_is_never_reported_as_quiescent_and_drop_does_not_panic() {
        let tracker = ProjectActivityTracker::default();
        let lease = tracker.enter(1).unwrap();
        let counts = Arc::clone(&tracker.counts);
        assert!(
            std::thread::spawn(move || {
                let _locked = counts.lock().unwrap();
                panic!("intentional poison");
            })
            .join()
            .is_err()
        );
        drop(lease);
        assert!(tracker.is_active(1));
        assert!(tracker.is_active(2));
        assert!(tracker.any_active());
        assert!(tracker.enter(2).is_err());
    }
}
