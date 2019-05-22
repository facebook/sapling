// Copyright Facebook, Inc. 2019

use std::time::Duration;

#[derive(Debug)]
pub struct DownloadStats {
    pub downloaded: usize,
    pub uploaded: usize,
    pub requests: usize,
    pub time: Duration,
}
