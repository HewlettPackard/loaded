use crate::util::{format_duration, format_duration_f64};
use bigdecimal::{BigDecimal, ToPrimitive};
use bytesize::ByteSize;
use hdrhistogram::Histogram;
use hyper::StatusCode;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

#[derive(Debug, Serialize)]
pub struct SummaryStats {
    total_runtime_ns: BigDecimal,
    total_bytes_written: BigDecimal,
    total_bytes_read: BigDecimal,
    total_reqs: BigDecimal,
    mean_reqs_per_second: BigDecimal,
    mean_bytes_written_per_second: BigDecimal,
    mean_bytes_read_per_second: BigDecimal,
    errors: HashMap<u16, usize>,
    round_trip_time_latency: LatencyStats,
    time_to_first_byte_latency: LatencyStats,
}

impl SummaryStats {
    //noinspection RsUnresolvedReference
    pub(crate) fn new(
        total_runtime_ns: BigDecimal,
        total_bytes_written: BigDecimal,
        total_bytes_read: BigDecimal,
        total_reqs: BigDecimal,
        stats: RunStats,
    ) -> Self {
        let ns_to_sec_factor = BigDecimal::from(10_i32.pow(9));
        let mean_reqs_per_second = (&total_reqs / &total_runtime_ns * &ns_to_sec_factor).round(6);

        let mean_bytes_written_per_second =
            (&total_bytes_written / (&total_runtime_ns / &ns_to_sec_factor)).round(6);
        let mean_bytes_read_per_second =
            (&total_bytes_read / (&total_runtime_ns / &ns_to_sec_factor)).round(6);

        SummaryStats {
            total_runtime_ns,
            total_bytes_written,
            total_bytes_read,
            total_reqs,
            mean_reqs_per_second,
            mean_bytes_written_per_second,
            mean_bytes_read_per_second,
            errors: stats.errors,
            round_trip_time_latency: stats.rtt_latency_hist.into(),
            time_to_first_byte_latency: stats.ttfb_latency_hist.into(),
        }
    }
}

impl Display for SummaryStats {
    //noinspection RsUnresolvedReference
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let ns_to_sec_factor = BigDecimal::from(10_i32.pow(9));

        f.write_str(&format!(
            "Total Runtime: {:.3}s\n",
            &self.total_runtime_ns / &ns_to_sec_factor
        ))?;
        f.write_str(&format!(
            "Total Requests: {}, Total Bytes Written: {:.3}, Total Bytes Read: {:.3}\n",
            self.total_reqs,
            ByteSize::b((&self.total_bytes_written).to_u64().unwrap()).to_string_as(true),
            ByteSize::b((&self.total_bytes_read).to_u64().unwrap()).to_string_as(true)
        ))?;
        f.write_str(&format!(
            "Mean Requests/s: {:.2}, Mean Bytes Written/s: {:.3}, Mean Bytes Read/s: {:.3}\n",
            ByteSize::b(self.mean_reqs_per_second.to_u64().unwrap()).to_string_as(true),
            ByteSize::b((&self.mean_bytes_written_per_second).to_u64().unwrap()).to_string_as(true),
            ByteSize::b((&self.mean_bytes_read_per_second).to_u64().unwrap()).to_string_as(true),
        ))?;

        let total_errors = self.errors.iter().fold(0, |acc, (_, v)| acc + *v);
        f.write_str(&format!("Errors: {total_errors}\n"))?;
        if !self.errors.is_empty() {
            for (k, v) in &self.errors {
                f.write_str(&format!(
                    "\t{} ({}): {}\n",
                    k,
                    StatusCode::from_u16(*k)
                        .unwrap()
                        .canonical_reason()
                        .unwrap(),
                    v
                ))?;
            }
        }

        f.write_str("Time to First Byte (TTFB) Latency Statistics:\n")?;
        f.write_str(&format!("{}", self.time_to_first_byte_latency))?;
        f.write_str("\r\n")?;
        f.write_str("Round Trip Time (RTT) Latency Statistics:\n")?;
        f.write_str(&format!("{}", self.round_trip_time_latency))?;

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct LatencyStats {
    mean: f64,
    min: u64,
    max: u64,
    p50: u64,
    p95: u64,
    p99: u64,
    p999: u64,
    p9999: u64,
}

impl Display for LatencyStats {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Mean: {}, Min: {}, Max: {}\n",
            format_duration_f64(self.mean),
            format_duration(self.min.into()),
            format_duration(self.max.into())
        ))?;
        f.write_str(&format!("p50: {}\n", format_duration(self.p50.into())))?;
        f.write_str(&format!("p95: {}\n", format_duration(self.p95.into())))?;
        f.write_str(&format!("p99: {}\n", format_duration(self.p99.into())))?;
        f.write_str(&format!("p999: {}\n", format_duration(self.p999.into())))?;
        f.write_str(&format!("p9999: {}\n", format_duration(self.p9999.into())))
    }
}

impl From<Histogram<u64>> for LatencyStats {
    fn from(value: Histogram<u64>) -> Self {
        LatencyStats {
            mean: value.mean(),
            min: value.min(),
            max: value.max(),
            p50: value.value_at_quantile(0.50),
            p95: value.value_at_quantile(0.95),
            p99: value.value_at_quantile(0.99),
            p999: value.value_at_quantile(0.999),
            p9999: value.value_at_quantile(0.9999),
        }
    }
}

#[derive(Debug)]
pub struct WorkerStats {
    pub instant_stats: InstantStats,
    pub run_stats: RunStats,
}

impl Default for WorkerStats {
    fn default() -> Self {
        WorkerStats {
            instant_stats: InstantStats::default(),
            run_stats: RunStats {
                errors: HashMap::new(),
                rtt_latency_hist: Histogram::new(3).unwrap(),
                ttfb_latency_hist: Histogram::new(3).unwrap(),
            },
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct InstantStats {
    pub requests_issued: usize,
    pub bytes_written: usize,
    pub bytes_read: usize,
}

impl InstantStats {
    #[must_use]
    pub fn changed(&self, since: &InstantStats) -> InstantStats {
        let requests_issued = changed(since.requests_issued, self.requests_issued);
        let bytes_written = changed(since.bytes_written, self.bytes_written);
        let bytes_read = changed(since.bytes_read, self.bytes_read);

        InstantStats {
            requests_issued,
            bytes_written,
            bytes_read,
        }
    }
}

#[derive(Debug)]
pub struct RunStats {
    pub errors: HashMap<u16, usize>,
    pub rtt_latency_hist: Histogram<u64>,
    pub ttfb_latency_hist: Histogram<u64>,
}

impl Default for RunStats {
    fn default() -> Self {
        RunStats {
            errors: HashMap::new(),
            rtt_latency_hist: Histogram::new(3).unwrap(),
            ttfb_latency_hist: Histogram::new(3).unwrap(),
        }
    }
}

fn changed(prev: usize, curr: usize) -> usize {
    if curr >= prev {
        curr - prev
    } else {
        // wrapped
        (usize::MAX - prev) + curr
    }
}
