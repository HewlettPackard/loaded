//! # Simple Engine
//!

use crate::engine::Engine;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt, Either, Empty, Full};
use hyper::body::Incoming;
use hyper::http::request::Builder;
use hyper::{Request, Response};

/// An simple engine to generate loads to any given server. This workload
/// consists of a single type of request, specifying the HTTP method,
/// HTTP headers and body (if any).
pub struct SimpleEngine {
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
}

#[async_trait(? Send)]
impl Engine<Either<Full<Bytes>, Empty<Bytes>>> for SimpleEngine {
    fn name<'a>(&self) -> &'a str {
        "simple"
    }

    async fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    async fn request(
        &mut self,
        req: Builder,
    ) -> Result<(Request<Either<Full<Bytes>, Empty<Bytes>>>, usize)> {
        let mut req = req.method(self.method.as_str());

        for (k, v) in &self.headers {
            req = req.header(k, v);
        }

        let req = match &self.body {
            None => req.body(Either::Right(Empty::new())),
            Some(r) => req.body(Either::Left(Full::new(r.clone()))),
        }
        .unwrap();

        Ok((req, self.body.as_ref().map_or_else(|| 0_usize, Bytes::len)))
    }

    async fn response(&mut self, resp: &mut Response<Incoming>) -> Result<usize> {
        let mut read = 0;
        while let Some(next) = resp.frame().await {
            let frame = next.unwrap();
            if let Some(d) = frame.data_ref() {
                read += d.len();
            }
        }
        Ok(read)
    }

    async fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}
