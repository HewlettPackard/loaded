use crate::connection::ConnectionLifecycle;
use async_trait::async_trait;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// A completion condition that marks the run as completed
/// once the specified number of requests have been issued
pub struct RequestCompletionCondition {
    local_run: Rc<AtomicBool>,
    num_requests: usize,
    num_requests_for_completion: usize,
}

impl RequestCompletionCondition {
    pub fn new(local_run: Rc<AtomicBool>, num_requests_for_completion: usize) -> Self {
        RequestCompletionCondition {
            local_run,
            num_requests: 0,
            num_requests_for_completion,
        }
    }
}

#[async_trait(? Send)]
impl ConnectionLifecycle for RequestCompletionCondition {
    async fn should_issue_request(&mut self) -> bool {
        self.num_requests += 1;
        if self.num_requests == self.num_requests_for_completion + 1 {
            self.local_run.store(false, SeqCst);
            false
        } else {
            true
        }
    }
}

/// A completion condition that marks the run as completed
/// once the specified duration has elapsed
pub struct DurationCompletionCondition {
    pub run: Arc<AtomicBool>,
    pub duration_cond: Duration,
    pub handle: Option<JoinHandle<()>>,
}

#[async_trait(? Send)]
impl ConnectionLifecycle for DurationCompletionCondition {
    async fn after_setup(&mut self) {
        let run_flag = self.run.clone();
        let duration = self.duration_cond;
        self.handle.replace(tokio::task::spawn_local(async move {
            sleep(duration).await;
            run_flag.store(false, Relaxed);
        }));
    }
}
