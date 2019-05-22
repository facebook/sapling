// Copyright Facebook, Inc. 2019

use std::{fmt, time::Duration};

#[derive(Debug)]
pub struct DownloadStats {
    pub downloaded: usize,
    pub uploaded: usize,
    pub requests: usize,
    pub time: Duration,
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
        write!(
            f,
            "Downloaded {} bytes in {:.3} seconds over {} request(s) ({:.2} MB/s)",
            self.downloaded,
            self.time_in_seconds(),
            self.requests,
            rate
        )
    }
}
