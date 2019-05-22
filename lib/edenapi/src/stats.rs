// Copyright Facebook, Inc. 2019

use std::{fmt, time::Duration};

#[derive(Debug)]
pub struct DownloadStats {
    pub downloaded: usize,
    pub uploaded: usize,
    pub requests: usize,
    pub time: Duration,
}

impl fmt::Display for DownloadStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let seconds =
            self.time.as_secs() as f64 + self.time.subsec_nanos() as f64 / 1_000_000_000.0;
        let rate = self.downloaded as f64 / 1_000_000.0 / seconds;
        write!(
            f,
            "Downloaded {} bytes in {:.3} seconds over {} request(s) ({:.2} MB/s)",
            self.downloaded, seconds, self.requests, rate
        )
    }
}
