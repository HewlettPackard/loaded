use anyhow::Result;
use async_trait::async_trait;
use hyper::body::{Body, Incoming};
use hyper::http::request;
use hyper::{Request, Response};

pub mod s3_engine;
pub mod simple_engine;

/// An engine for generating http traffic to be sent to a HTTP server via an [crate::connection::Connection]
#[async_trait(? Send)]
pub trait Engine<Req>
where
    Req: Body,
{
    /// Name of the engine type
    fn name<'a>(&self) -> &'a str;
    /// Performs whatever setup is necessary for the engine
    ///
    /// Only called once at the start of the run
    async fn setup(&mut self) -> Result<()>;
    /// Builds up a request to be issued by the parent [crate::connection::Connection]
    ///
    /// Request builder will already fill in the following:
    /// - Uri
    /// - Authority (Header, derived from url)
    async fn request(&mut self, req: request::Builder) -> Result<(Request<Req>, usize)>;
    /// Parses a response returning the size of the read payload
    async fn response(&mut self, resp: &mut Response<Incoming>) -> Result<usize>;
    /// Performs whatever cleanup is necessary for the engine before exiting
    ///
    /// Called once at the end of a run
    async fn cleanup(&mut self) -> Result<()>;
}
