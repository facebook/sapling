// Copyright Facebook, Inc. 2019

use std::{fmt, time::Duration};

#[derive(Debug)]
pub struct DownloadStats {
    pub downloaded: usize,
    pub uploaded: usize,
    pub requests: usize,
    pub time: Duration,
    pub latency: Duration,
}

impl DownloadStats {
    pub fn time_in_seconds(&self) -> f64 {
        duration_to_seconds(self.time)
    }

    pub fn bytes_per_second(&self) -> f64 {
        self.downloaded as f64 / self.time_in_seconds()
    }
}

impl fmt::Display for DownloadStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} downloaded in {} over {} request{} ({:.2} MB/s; latency: {})",
            fmt_num_bytes(self.downloaded),
            fmt_duration(self.time),
            self.requests,
            if self.requests == 1 { "" } else { "s" },
            self.bytes_per_second() / 1_000_000.0,
            fmt_duration(self.latency)
        )
    }
}

fn fmt_num_bytes(n: usize) -> String {
    if n == 0 {
        return "0 B".into();
    }
    let mut n = n as f64;
    let i = (n.log10() / 3.0).floor() as usize;
    n /= 1000f64.powi(i as i32);
    let units = ["B", "kB", "MB", "GB"];
    let prec = if i > 0 { 2 } else { 0 };
    format!("{:.*} {}", prec, n, units[i])
}

fn fmt_duration(time: Duration) -> String {
    let millis = time.as_millis();
    if millis > 1000 {
        format!("{:.2} s", duration_to_seconds(time))
    } else {
        format!("{} ms", millis)
    }
}

fn duration_to_seconds(time: Duration) -> f64 {
    time.as_secs() as f64 + time.subsec_nanos() as f64 / 1_000_000_000.0
}
