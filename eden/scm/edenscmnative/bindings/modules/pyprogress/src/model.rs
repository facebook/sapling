/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module exposes Rust's progress model to Python.

use std::sync::Arc;

use cpython::*;
use cpython_ext::PyNone;
use progress_model::CacheStats as CacheStatsModel;
use progress_model::ProgressBar as ProgressBarModel;
use progress_model::Registry;

py_class!(pub class ProgressBar |py| {
    data model: Arc<ProgressBarModel>;

    def __new__(
        _cls,
        topic: String,
        total: Option<u64> = None,
        unit: Option<String> = None
    ) -> PyResult<Self> {
        let unit = unit.clone().unwrap_or_default();
        let total = total.unwrap_or_default();
        let bar = ProgressBarModel::register_new(topic, total, unit);
        Self::create_instance(py, bar)
    }

    def position_total(&self) -> PyResult<(u64, u64)> {
        Ok(self.model(py).position_total())
    }

    def set_position(&self, value: u64) -> PyResult<PyNone> {
        self.model(py).set_position(value);
        Ok(PyNone)
    }

    def set_total(&self, value: u64) -> PyResult<PyNone> {
        self.model(py).set_total(value);
        Ok(PyNone)
    }

    def increase_position(&self, value: u64) -> PyResult<PyNone> {
        self.model(py).increase_position(value);
        Ok(PyNone)
    }

    def increase_total(&self, value: u64) -> PyResult<PyNone> {
        self.model(py).increase_total(value);
        Ok(PyNone)
    }

    def set_message(&self, message: String) -> PyResult<PyNone> {
        self.model(py).set_message(message);
        Ok(PyNone)
    }
});

py_class!(pub class CacheStats |py| {
    data model: Arc<CacheStatsModel>;

    def __new__(
        _cls,
        topic: String,
    ) -> PyResult<Self> {
        let model = CacheStatsModel::new(topic);
        Registry::main().register_cache_stats(&model);
        Self::create_instance(py, model)
    }

    def increase_hit(&self, value: usize = 1) -> PyResult<PyNone> {
        self.model(py).increase_hit(value);
        Ok(PyNone)
    }

    def increase_miss(&self, value: usize = 1) -> PyResult<PyNone> {
        self.model(py).increase_miss(value);
        Ok(PyNone)
    }
});
