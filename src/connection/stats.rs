use crate::connection::lifecycle::ConnectionLifecycle;
use crate::stats::WorkerStats;
use async_trait::async_trait;
use hyper::{Request, Response};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::Instant;

/// A stats collector that hooks into the connection lifecycle to gather
/// related statistics
pub struct StatsCollector {
    pub stats: Arc<RwLock<WorkerStats>>,
    req_size: usize,
    start: Option<Instant>,
    time_to_first_byte: Option<Duration>,
}

impl StatsCollector {
    pub fn new(stats: Arc<RwLock<WorkerStats>>) -> Self {
        StatsCollector {
            stats,
            req_size: 0,
            start: None,
            time_to_first_byte: None,
        }
    }
}

#[async_trait(?Send)]
impl ConnectionLifecycle for StatsCollector {
    async fn before_request<T>(&mut self, _req: &Request<T>, req_size: usize) {
        self.start.replace(Instant::now());
        self.req_size = req_size;
    }

    async fn after_request(&mut self) {
        self.time_to_first_byte
            .replace(self.start.unwrap().elapsed());
    }

    async fn after_response<T>(&mut self, resp: &Response<T>, resp_len: usize) {
        let mut guard = self.stats.write().await;
        if resp.status().is_success() {
            let round_trip_time = u64::try_from(self.start.unwrap().elapsed().as_nanos()).unwrap();
            guard
                .run_stats
                .rtt_latency_hist
                .record(round_trip_time)
                .unwrap();
            guard
                .run_stats
                .ttfb_latency_hist
                .record(u64::try_from(self.time_to_first_byte.unwrap().as_nanos()).unwrap())
                .unwrap();
            guard.instant_stats.requests_issued += 1;
            guard.instant_stats.bytes_written += self.req_size;
            guard.instant_stats.bytes_read += resp_len;
        } else {
            guard
                .run_stats
                .errors
                .entry(resp.status().as_u16())
                .and_modify(|v| *v += 1_usize)
                .or_insert(1);
        }
        drop(guard);
    }
}
