//! # Connection
//!
//! A connection is responsible for maintaining an underlying tcp connection
//! and repeatedly issuing http requests to the http server. These requests are
//! formed by calling engine methods that define what requests are sent and what is to
//! be done with the response.

use crate::connection::lifecycle::{ConnectionHttpLifecycle, ConnectionLifecycle};
use crate::engine::Engine;
use anyhow::Result;
use hyper::body::Body;
use hyper::{Request, Uri};
use log::{error, info, trace};
use std::error::Error;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Barrier;
use tokio::time::Instant;

pub mod completion;
pub mod lifecycle;
pub mod rate_limit;
pub mod stats;

pub struct Connection {
    pub parent_worker_id: usize,
    pub id: usize,
    pub run_flag: RunFlag,
    pub setup_barrier: Arc<Barrier>,
    pub lifecycle_listeners: Vec<ConnectionHttpLifecycle>,
}

pub struct ConnectionRunInfo {
    pub start_time: Instant,
    pub end_time: Instant,
}

impl Connection {
    pub async fn run<E, Req>(&mut self, engine: &mut E, url: &Uri) -> Result<ConnectionRunInfo>
    where
        E: Engine<Req>,
        Req: Body + Send + 'static,
        Req::Data: Send,
        Req::Error: Into<Box<dyn Error + Send + Sync>>,
    {
        info!(
            "Starting {} engine (worker {}, connection: {})",
            engine.name(),
            self.parent_worker_id,
            self.id
        );
        engine.setup().await?;

        self.setup_barrier.wait().await;

        for l in &mut self.lifecycle_listeners {
            l.after_setup().await;
        }

        let host = url.host().expect("uri has no host");
        let port = url.port_u16().unwrap_or(80);
        let address = format!("{host}:{port}");

        let stream = TcpStream::connect(address).await?;

        let (mut sender, conn) = hyper::client::conn::http1::handshake(stream).await.unwrap();

        tokio::task::spawn_local(async move {
            if let Err(err) = conn.await {
                error!("Connection failed: {:?}", err);
            }
        });

        let start_time = Instant::now();
        let authority = url.authority().unwrap().clone();

        'run: loop {
            if !self.run_flag.should_run() {
                break;
            }

            for l in &mut self.lifecycle_listeners {
                if !l.should_issue_request().await {
                    continue 'run;
                }
            }

            // Create an HTTP request with an empty body and a HOST header
            let builder = Request::builder()
                .uri(url)
                .header(hyper::header::HOST, authority.as_str());

            let (req, req_len) = engine.request(builder).await?;

            for l in &mut self.lifecycle_listeners {
                l.before_request(&req, req_len).await;
            }

            trace!("Sending request {} - {} ", req.method(), req.uri());
            let mut resp = sender.send_request(req).await?;

            for l in &mut self.lifecycle_listeners {
                l.after_request().await;
            }

            let len = engine.response(&mut resp).await?;

            for l in &mut self.lifecycle_listeners {
                l.after_response(&resp, len).await;
            }
        }

        let end_time = Instant::now();

        info!("Cleaning up {} engine ({})", engine.name(), self.id);
        engine.cleanup().await?;

        Ok(ConnectionRunInfo {
            start_time,
            end_time,
        })
    }
}

#[derive(Debug)]
pub struct RunFlag {
    global_run: Arc<AtomicBool>,
    local_run: Rc<AtomicBool>,
}

impl RunFlag {
    pub fn new(global_run: Arc<AtomicBool>, local_run: Rc<AtomicBool>) -> Self {
        RunFlag {
            global_run,
            local_run,
        }
    }

    fn should_run(&self) -> bool {
        self.global_run.load(Relaxed) && self.local_run.load(Relaxed)
    }
}
