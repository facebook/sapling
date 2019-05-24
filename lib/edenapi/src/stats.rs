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
        self.time.as_secs() as f64 + self.time.subsec_nanos() as f64 / 1_000_000_000.0
    }

    pub fn bytes_per_second(&self) -> f64 {
        self.downloaded as f64 / self.time_in_seconds()
    }
}

impl fmt::Display for DownloadStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let rate = self.bytes_per_second() / 1_000_000.0; // Convert to MB/s.
        let time = self.time_in_seconds();
        let (time, prec, unit) = if time > 1.0 {
            (time, 2, "s")
        } else {
            (time * 1000.0, 0, "ms")
        };
        write!(
            f,
            "{} downloaded in {:.*} {} over {} request{} ({:.2} MB/s)",
            fmt_num_bytes(self.downloaded),
            prec,
            time,
            unit,
            self.requests,
            if self.requests == 1 { "" } else { "s" },
            rate
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
