use crate::cli::{FormatType, RunCmd};
use crate::stats::{InstantStats, RunStats, SummaryStats, WorkerStats};
use crate::worker::{CompletionCondition, Worker, WorkerInfo};
use anyhow::{anyhow, Result};
use bigdecimal::BigDecimal;
use bytesize::ByteSize;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use log::{error, info};
use num_bigint::BigInt;

use crate::util;
use itertools::izip;
use std::iter::zip;
use std::num::NonZeroU32;
use std::process::exit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use std::{iter, thread};
use tokio::sync::RwLock;
use tokio::time::Instant;

pub fn run(args: &RunCmd) -> Result<()> {
    let run_flag = Arc::new(AtomicBool::new(true));
    let run_flag_c = run_flag.clone();
    ctrlc::set_handler(move || {
        run_flag_c.store(false, Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let mut handles = vec![];
    let mut stats = vec![];

    info!("Starting {} workers: ", args.threads);

    let lim = args.rate_limit.map(|rate| {
        Arc::new(RateLimiter::direct(Quota::per_second(
            NonZeroU32::new(rate).unwrap(),
        )))
    });

    // Build the completions conditions that correspond to our workers
    let completion_conditions: Vec<Option<CompletionCondition>> = if args.num_requests.is_some() {
        // Divvy up the requests across the workers so they're distributed evenly
        util::divvy(args.num_requests.unwrap(), args.threads)
            .map(|num_requests| Some(CompletionCondition::NumRequests(num_requests)))
            .collect()
    } else if args.duration.is_some() {
        iter::repeat(Some(CompletionCondition::Duration(args.duration.unwrap())))
            .take(args.threads)
            .collect()
    } else {
        iter::repeat(None).take(args.threads).collect()
    };

    for (i, num_connections, completion_condition) in izip!(
        0..args.threads,
        util::divvy(args.connections, args.threads),
        completion_conditions,
    ) {
        let worker_stats = Arc::new(RwLock::new(WorkerStats::default()));
        let handle = start_worker(
            &args,
            num_connections,
            &run_flag,
            &lim,
            &completion_condition,
            i,
            &worker_stats,
        )?;

        handles.push(handle);
        stats.push(worker_stats);
    }

    let (requests_issued, bytes_written, bytes_read) =
        wait_for_completion(&args, &run_flag, &handles, &mut stats);

    let infos = handles
        .into_iter()
        .flat_map(JoinHandle::join)
        .filter_map(|res| match res {
            Ok(o) => Some(o),
            Err(e) => {
                error!("Worker encountered an error: {}", e);
                None
            }
        })
        .collect::<Vec<_>>();

    if infos.is_empty() {
        exit(1)
    }

    let total_runtime = get_total_runtime(infos);
    let summary_stats = SummaryStats::new(
        BigDecimal::from(total_runtime),
        bytes_written.into(),
        bytes_read.into(),
        requests_issued.into(),
        summarize_worker_stats(&stats)?,
    );

    match args.format {
        FormatType::Pretty => println!("{summary_stats}"),
        FormatType::Json => println!("{}", serde_json::to_string_pretty(&summary_stats)?),
    }

    Ok(())
}

fn start_worker(
    args: &RunCmd,
    connections: usize,
    run_flag: &Arc<AtomicBool>,
    lim: &Option<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
    completion_condition: &Option<CompletionCondition>,
    worker_id: usize,
    worker_stats: &Arc<RwLock<WorkerStats>>,
) -> Result<JoinHandle<Result<WorkerInfo>>> {
    let url = args.url.clone();
    info!("Starting worker {}", worker_id);

    let mut worker = Worker {
        worker_id,
        stats: worker_stats.clone(),
        run_flag: run_flag.clone(),
        rate_limit: lim.clone(),
    };
    let engine = args.engine.clone();
    let completion_condition = completion_condition.clone();
    let seed = args.seed.clone();
    let handle = thread::Builder::new()
        .name(format!("Worker {worker_id}"))
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build runtime");

            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                worker
                    .run(engine, url, connections, seed, completion_condition)
                    .await
            })
        })
        .map_err(|e| anyhow!("{:?}", e))?;

    Ok(handle)
}

fn wait_for_completion(
    args: &RunCmd,
    run_flag: &Arc<AtomicBool>,
    worker_handles: &[JoinHandle<Result<WorkerInfo>>],
    current_stats: &mut Vec<Arc<RwLock<WorkerStats>>>,
) -> (BigInt, BigInt, BigInt) {
    let dur = Duration::from_millis(1000);
    let mut previous_stats: Vec<InstantStats> = vec![];
    for _ in 0..args.threads {
        previous_stats.push(InstantStats::default());
    }
    let mut total_reqs: BigInt = BigInt::default();
    let mut total_bytes_written: BigInt = BigInt::default();
    let mut total_bytes_read: BigInt = BigInt::default();

    loop {
        if !run_flag.load(Relaxed) || worker_handles.iter().all(JoinHandle::is_finished) {
            break;
        }

        sleep(dur);

        let stats = sum_instant_stats(&mut previous_stats, &current_stats);
        total_reqs += stats.requests_issued;
        total_bytes_written += stats.bytes_written;
        total_bytes_read += stats.bytes_read;

        println!(
            "{} Req/s, Write/s: {}, Read/s: {}",
            stats.requests_issued,
            ByteSize::b(stats.bytes_written as u64).to_string_as(true),
            ByteSize::b(stats.bytes_read as u64).to_string_as(true)
        );
    }
    (total_reqs, total_bytes_written, total_bytes_read)
}

fn sum_instant_stats(
    curr: &mut Vec<InstantStats>,
    th: &Vec<Arc<RwLock<WorkerStats>>>,
) -> InstantStats {
    let mut stats = vec![];
    for (a, b) in zip(th, curr) {
        let guard = a.blocking_read();
        let changed = guard.instant_stats.changed(b);
        b.requests_issued = guard.instant_stats.requests_issued;
        b.bytes_written = guard.instant_stats.bytes_written;
        b.bytes_read = guard.instant_stats.bytes_read;

        drop(guard);

        stats.push(InstantStats {
            requests_issued: changed.requests_issued,
            bytes_written: changed.bytes_written,
            bytes_read: changed.bytes_read,
        });
    }

    stats.iter().fold(InstantStats::default(), |mut acc, curr| {
        acc.requests_issued += curr.requests_issued;
        acc.bytes_written += curr.bytes_written;
        acc.bytes_read += curr.bytes_read;
        acc
    })
}

fn summarize_worker_stats(th: &[Arc<RwLock<WorkerStats>>]) -> Result<RunStats> {
    th.iter().try_fold(RunStats::default(), |mut acc, curr| {
        let guard = curr.blocking_read();
        acc.rtt_latency_hist
            .add(&guard.run_stats.rtt_latency_hist)?;
        acc.ttfb_latency_hist
            .add(&guard.run_stats.ttfb_latency_hist)?;
        guard.run_stats.errors.iter().for_each(|(k, v)| {
            acc.errors
                .entry(*k)
                .and_modify(|val| *val += *v)
                .or_insert(*v);
        });
        Ok(acc)
    })
}

fn get_total_runtime(infos: Vec<WorkerInfo>) -> u128 {
    let mut earliest_start = Instant::now();
    let mut latest_end = Instant::now();
    for info in infos {
        earliest_start = info.run_infos.iter().fold(earliest_start, |acc, t| {
            if t.start_time < acc {
                t.start_time
            } else {
                acc
            }
        });
        latest_end = info.run_infos.iter().fold(earliest_start, |acc, t| {
            if t.end_time > acc {
                t.end_time
            } else {
                acc
            }
        });
    }

    let i1 = latest_end.duration_since(earliest_start).as_nanos();
    i1
}
