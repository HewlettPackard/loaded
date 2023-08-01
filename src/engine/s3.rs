mod traffic;
pub mod uri;

use crate::cli::TrafficPattern;
use crate::engine::Engine;
use crate::stream::checksum::Checksum;
use crate::stream::StreamProvider;
use crate::util;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Buf;
use chrono::Utc;
use futures::Stream;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::http::request;
use hyper::{Request, Response};
use log::warn;
use std::cell::RefCell;
use std::marker::PhantomData;
use traffic::{TrafficState, TrafficStateMachine};
use uri::UriProvider;

/// An S3 engine to generate http traffic to an S3 server. This workload
/// will consist of PUTs and GETs to the server.
///
/// This S3 engine does not make use of any provided S3 client but instead manually
/// crafts the requests to ensure control over the payload and push as much load
/// as possible.
pub struct S3Engine<P, S>
where
    P: StreamProvider<S>,
    S: Stream,
{
    stream_supplier: RefCell<P>,
    object_size: usize,
    phantom: PhantomData<S>,
    checksum_algo: Option<Checksum>,
    traffic_cop: TrafficStateMachine,
    last_traffic_state: Option<TrafficState>,
}

impl<P, S> S3Engine<P, S>
where
    P: StreamProvider<S>,
    S: Stream,
{
    pub fn new(
        stream_supplier: P,
        uri_supplier: UriProvider,
        object_size: usize,
        checksum_algo: Option<Checksum>,
        traffic_pattern: TrafficPattern,
    ) -> Self {
        S3Engine {
            stream_supplier: RefCell::new(stream_supplier),
            object_size,
            phantom: PhantomData,
            checksum_algo,
            traffic_cop: TrafficStateMachine::new(traffic_pattern, uri_supplier),
            last_traffic_state: None,
        }
    }
}

#[async_trait(? Send)]
impl<P, S, D, E> Engine<StreamBody<S>> for S3Engine<P, S>
where
    P: StreamProvider<S>,
    S: Stream<Item = Result<Frame<D>, E>>,
    D: Buf,
{
    fn name<'a>(&self) -> &'a str {
        "s3"
    }

    async fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    #[allow(clippy::await_holding_refcell_ref)]
    async fn request(&mut self, req: request::Builder) -> Result<(Request<StreamBody<S>>, usize)> {
        self.last_traffic_state = Some(self.traffic_cop.next());
        match self.last_traffic_state.as_ref().unwrap() {
            TrafficState::Put { uri } => {
                let (req, stream) = match &self.checksum_algo {
                    None => (req, self.stream_supplier.borrow_mut().new_stream()),
                    Some(c) => {
                        let (stream, digest) = self
                            .stream_supplier
                            .borrow_mut()
                            .new_stream_with_checksum(c)
                            .await;

                        let req = match c {
                            Checksum::Md5 => req.header("Content-MD5", digest),
                            Checksum::Crc32 => req.header("x-amz-checksum-crc32", digest),
                            Checksum::Crc32c => req.header("x-amz-checksum-crc32c", digest),
                            Checksum::Sha1 => req.header("x-amz-checksum-sha1", digest),
                            Checksum::Sha2 => req.header("x-amz-checksum-sha256", digest),
                        };
                        (req, stream)
                    }
                };

                let req = req
                    .uri(uri)
                    .method("PUT")
                    .header(hyper::header::USER_AGENT, util::user_agent())
                    .header(hyper::header::CONTENT_TYPE, "application/octet-stream")
                    .header(hyper::header::CONTENT_LENGTH, self.object_size.to_string())
                    .header(
                        "X-Amz-Date",
                        Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
                    )
                    .body(StreamBody::new(stream))?;

                Ok((req, self.object_size))
            }
            TrafficState::Get { uri } => {
                let req = req
                    .uri(uri)
                    .method("GET")
                    .header(hyper::header::ACCEPT, "application/octet-stream")
                    .body(StreamBody::new(self.stream_supplier.borrow_mut().empty()))?;
                Ok((req, 0))
            }
        }
    }

    async fn response(&mut self, resp: &mut Response<Incoming>) -> Result<usize> {
        let mut read = 0;
        while let Some(next) = resp.frame().await {
            let frame = next.unwrap();
            if let Some(d) = frame.data_ref() {
                read += d.len();
            }
        }

        if resp.status().is_success() {
            if let Some(TrafficState::Get { .. }) = self.last_traffic_state {
                if read != self.object_size {
                    warn!(
                        "Unexpected object size {read}, expected {}",
                        self.object_size
                    );
                }
            }
        }

        Ok(read)
    }

    async fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}
