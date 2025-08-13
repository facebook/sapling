/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use edenapi_ext::DownloadFileStatsSnapshot;

py_class!(pub class downloadfilestats |py| {
    data stats: DownloadFileStatsSnapshot;

    def __str__(&self) -> PyResult<String> {
        Ok(self.stats(py).to_string())
    }

    def total_blobs(&self) -> PyResult<usize> {
        Ok(self.stats(py).total_blobs())
    }

    def blobs_from_disk_state(&self) -> PyResult<usize> {
        Ok(self.stats(py).blobs_from_disk_state)
    }

    def blobs_from_local_cache(&self) -> PyResult<usize> {
        Ok(self.stats(py).blobs_from_local_cache)
    }

    def blobs_fetched_remotely(&self) -> PyResult<usize> {
        Ok(self.stats(py).blobs_fetched_remotely)
    }
});

impl downloadfilestats {
    pub fn new(py: Python, stats: DownloadFileStatsSnapshot) -> PyResult<Self> {
        Self::create_instance(py, stats)
    }
}
