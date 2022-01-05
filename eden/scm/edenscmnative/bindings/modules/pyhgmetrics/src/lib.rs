/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::Str;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "hgmetrics"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "incrementcounter",
        py_fn!(py, increment_counter(key: &str, value: usize = 1)),
    )?;
    m.add(py, "summarize", py_fn!(py, summarize()))?;
    Ok(m)
}

fn increment_counter(_py: Python, key: &str, value: usize) -> PyResult<PyNone> {
    hg_metrics::increment_counter(key, value);
    Ok(PyNone)
}

fn summarize(_py: Python) -> PyResult<HashMap<Str, usize>> {
    let summary = hg_metrics::summarize();
    let summary: HashMap<Str, usize> = summary.into_iter().map(|(k, v)| (k.into(), v)).collect();
    Ok(summary)
}
