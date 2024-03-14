use crate::cli::{Engine, S3Args, SimpleArgs};
use crate::connection::completion::{DurationCompletionCondition, RequestCompletionCondition};
use crate::connection::lifecycle::ConnectionHttpLifecycle;
use crate::connection::rate_limit::RateLimit;
use crate::connection::stats::StatsCollector;
use crate::connection::{Connection, ConnectionRunInfo, RunFlag};
use crate::engine::s3::uri::UriProvider;
use crate::engine::s3::S3Engine;
use crate::engine::simple::SimpleEngine;
use crate::stats::WorkerStats;
use crate::stream::perpetual_stream::PerpetualByteStreamSupplier;
use crate::util;
use anyhow::Result;
use bytes::{Bytes, BytesMut};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::RateLimiter;
use hyper::Uri;
use log::debug;
use std::iter;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::{Barrier, RwLock};

pub struct Worker {
    pub worker_id: usize,
    pub run_flag: Arc<AtomicBool>,
    pub stats: Arc<RwLock<WorkerStats>>,
    pub rate_limit: Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
}

pub struct WorkerInfo {
    pub worker_id: usize,
    pub run_infos: Vec<ConnectionRunInfo>,
}

impl Worker {
    pub async fn run(
        &mut self,
        engine: Engine,
        url: String,
        num_connections: usize,
        seed: String,
        completion_condition: Option<CompletionCondition>,
    ) -> Result<WorkerInfo> {
        debug!(
            "Running worker {} with {num_connections} connections",
            self.worker_id
        );
        let mut handles = vec![];

        // Setup barrier to sync up all connections to not proceed until all have
        // completed their setup step
        let setup_barrier = Arc::new(Barrier::new(num_connections));

        // Build the completions conditions that correspond to our connections
        let completion_conditions: Vec<Option<CompletionCondition>> = match completion_condition {
            None => iter::repeat(None).take(num_connections).collect(),
            Some(c) => {
                if let CompletionCondition::NumRequests(r) = c {
                    // Divvy up the requests across the connections so they're distributed evenly
                    util::divvy(r, num_connections)
                        .map(|num_requests| Some(CompletionCondition::NumRequests(num_requests)))
                        .collect()
                } else {
                    iter::repeat(Some(c)).take(num_connections).collect()
                }
            }
        };

        for (i, completion_condition) in iter::zip(0..num_connections, completion_conditions) {
            let url = url.parse::<Uri>()?;
            let stats = self.stats.clone();
            let run = self.run_flag.clone();
            let barrier = setup_barrier.clone();
            let limit = self.rate_limit.clone();
            let engine = engine.clone();
            let seed = seed.clone();
            let parent_worker_id = self.worker_id;

            let handle = tokio::task::spawn_local(async move {
                let local_run = Rc::new(AtomicBool::new(true));
                let lifecycle_listeners = Self::create_lifecycle_listeners(
                    i,
                    stats,
                    &run,
                    &local_run,
                    limit,
                    completion_condition,
                );

                let connection = Connection {
                    parent_worker_id,
                    run_flag: RunFlag::new(run, local_run),
                    setup_barrier: barrier,
                    id: i,
                    lifecycle_listeners,
                };

                match engine {
                    Engine::Simple(simple_args) => {
                        Self::run_simple_engine(connection, &url, simple_args).await?
                    }
                    Engine::S3(s3_args) => {
                        Self::run_s3_engine(connection, &url, seed, s3_args).await?
                    }
                }
            });
            handles.push(handle);
        }

        debug!("Waiting for worker {} to complete", self.worker_id);
        let mut run_infos = vec![];
        for h in handles {
            run_infos.push(h.await??);
        }
        debug!("Worker {} completed", self.worker_id);

        Ok(WorkerInfo {
            worker_id: self.worker_id,
            run_infos,
        })
    }

    fn create_lifecycle_listeners(
        id: usize,
        stats: Arc<RwLock<WorkerStats>>,
        global_run: &Arc<AtomicBool>,
        local_run: &Rc<AtomicBool>,
        limit: Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
        completion_condition: Option<CompletionCondition>,
    ) -> Vec<ConnectionHttpLifecycle> {
        let mut lifecycle_listeners =
            vec![ConnectionHttpLifecycle::Stats(StatsCollector::new(stats))];
        if let Some(l) = limit {
            lifecycle_listeners.push(ConnectionHttpLifecycle::RateLimit(RateLimit::new(l)));
        }
        if let Some(cond) = completion_condition {
            match cond {
                CompletionCondition::NumRequests(num_requests) => {
                    lifecycle_listeners.push(ConnectionHttpLifecycle::RequestsCompletion(
                        RequestCompletionCondition::new(local_run.clone(), num_requests),
                    ));
                }
                CompletionCondition::Duration(duration) => {
                    if id == 0 {
                        // only run one of these
                        lifecycle_listeners.push(ConnectionHttpLifecycle::DurationCompletion(
                            DurationCompletionCondition {
                                run: global_run.clone(),
                                duration_cond: duration,
                                handle: None,
                            },
                        ));
                    }
                }
            }
        }
        lifecycle_listeners
    }

    async fn run_simple_engine(
        mut connection: Connection,
        url: &Uri,
        simple_args: SimpleArgs,
    ) -> Result<Result<ConnectionRunInfo>> {
        let body = if simple_args.body_from_file.is_some() {
            let mut buf = Vec::new();
            let mut file = File::open(&simple_args.body_from_file.unwrap()).await?;
            file.read_to_end(&mut buf).await?;
            Some(Bytes::from(buf))
        } else if simple_args.body.is_some() {
            Some(Bytes::from(simple_args.body.unwrap()))
        } else {
            None
        };

        let mut engine = SimpleEngine {
            method: simple_args.method,
            headers: simple_args.headers,
            body,
        };

        Ok(connection.run(&mut engine, url).await)
    }

    async fn run_s3_engine(
        mut connection: Connection,
        url: &Uri,
        _seed: String,
        s3_args: S3Args,
    ) -> Result<Result<ConnectionRunInfo>> {
        let mut file = File::open("/dev/urandom").await?;
        let mut bytes = BytesMut::zeroed(1024 * 128);
        file.read_exact(&mut bytes).await?;

        let bytes = bytes.freeze();

        let base = format!(
            "{}://{}:{}",
            &url.scheme().unwrap(),
            &url.host().unwrap(),
            &url.port().unwrap()
        );

        let uri_supplier = UriProvider::new(
            base,
            s3_args.bucket,
            s3_args.obj_prefix,
            s3_args.prefix_folder_depth,
            s3_args.num_objs_per_prefix_folder,
            s3_args.num_branches_per_folder_depth,
        );

        let mut engine = if let Some(c) = s3_args.checksum_algorithm {
            let supp =
                PerpetualByteStreamSupplier::with_checksums(bytes, 0, s3_args.object_size, &[c])
                    .await;

            S3Engine::new(
                supp,
                uri_supplier,
                s3_args.object_size,
                Some(c),
                s3_args.traffic_pattern,
            )
        } else {
            let supp = PerpetualByteStreamSupplier::new(bytes, 0, s3_args.object_size);

            S3Engine::new(
                supp,
                uri_supplier,
                s3_args.object_size,
                None,
                s3_args.traffic_pattern,
            )
        };

        Ok(connection.run(&mut engine, url).await)
    }
}

#[derive(Debug, Clone)]
pub enum CompletionCondition {
    NumRequests(usize),
    Duration(Duration),
}
