use crate::connection::ConnectionLifecycle;
use async_trait::async_trait;
use governor::clock::{Clock, DefaultClock, QuantaClock};
use governor::state::{InMemoryState, NotKeyed};
use governor::RateLimiter;
use std::sync::Arc;
use tokio::time::sleep;

/// A rate limiter that hooks into the lifecycle of a connection to only allow
/// certain number of requests to be issued.
///
/// Currently, the `self.limiter` is shared across all connections to ensure we
/// are correctly limiting the entire run. Since we're already reducing the throughput,
/// a performance drop due to shared resources isn't a huge concern.
pub struct RateLimit {
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    clock: QuantaClock,
}

impl RateLimit {
    pub fn new(limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>) -> Self {
        RateLimit {
            limiter,
            clock: QuantaClock::default(),
        }
    }
}

#[async_trait(?Send)]
impl ConnectionLifecycle for RateLimit {
    async fn should_issue_request(&mut self) -> bool {
        if let Err(e) = self.limiter.check() {
            sleep(e.wait_time_from(self.clock.now())).await;
            false
        } else {
            true
        }
    }
}
