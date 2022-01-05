/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use edenapi::Stats;

py_class!(pub class stats |py| {
    data stats: Stats;

    def __str__(&self) -> PyResult<String> {
        Ok(self.stats(py).to_string())
    }

    def downloaded(&self) -> PyResult<usize> {
        Ok(self.stats(py).downloaded)
    }

    def uploaded(&self) -> PyResult<usize> {
        Ok(self.stats(py).uploaded)
    }

    def requests(&self) -> PyResult<usize> {
        Ok(self.stats(py).requests)
    }

    def time_in_seconds(&self) -> PyResult<f64> {
        Ok(self.stats(py).time_in_seconds())
    }

    def time_in_millis(&self) -> PyResult<usize> {
        Ok(self.stats(py).time.as_millis() as usize)
    }

    def latency_in_millis(&self) -> PyResult<usize> {
        Ok(self.stats(py).latency.as_millis() as usize)
    }

    def bytes_per_second(&self) -> PyResult<f64> {
        Ok(self.stats(py).bytes_per_second())
    }
});

impl stats {
    pub fn new(py: Python, stats: Stats) -> PyResult<Self> {
        Self::create_instance(py, stats)
    }
}
