use crate::connection::completion::{DurationCompletionCondition, RequestCompletionCondition};
use crate::connection::rate_limit::RateLimit;
use crate::connection::stats::StatsCollector;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use hyper::{Request, Response};

/// Hook into the lifecycle of a Connection
///
/// ```text
///                  ┌─┐
///                  └┼┘
///                   ▼
///              after_setup
///                   │
///                   ▼
///     ┌───► should_issue_request ────┐
///     │                              ▼
/// after_response             before_request
///     ▲                              │
///     └─────── after_request ◄───────┘
/// ```
#[async_trait(? Send)]
#[enum_dispatch]
#[allow(unused_variables, unused_mut)]
pub trait ConnectionLifecycle {
    /// Called once after Engine::setup() has been successfully called
    async fn after_setup(&mut self) {}
    /// Called before building a request
    async fn should_issue_request(&mut self) -> bool {
        true
    }
    /// Called before issuing a request
    async fn before_request<T>(&mut self, req: &Request<T>, req_size: usize) {}
    /// Called after issuing a request but before the engine handles the response
    async fn after_request(&mut self) {}
    /// Called after an engine has handled the response
    async fn after_response<T>(&mut self, resp: &Response<T>, resp_len: usize) {}
}

#[enum_dispatch(ConnectionLifecycle)]
pub enum ConnectionHttpLifecycle {
    Stats(StatsCollector),
    RateLimit(RateLimit),
    DurationCompletion(DurationCompletionCondition),
    RequestsCompletion(RequestCompletionCondition),
}
