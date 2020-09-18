/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Lightweight utilities. To optimize compile time, do not put complex
//! functions or functions depending on other crates here.

use cpython::{py_fn, PyModule, PyResult, Python};
use cpython_ext::PyNone;

/// Populate an existing module so it contains utilities.
pub(crate) fn populate_module(py: Python<'_>, module: &PyModule) -> PyResult<PyNone> {
    module.add(
        py,
        "sleep",
        py_fn!(py, sleep(seconds: f64, gil: bool = false)),
    )?;
    Ok(PyNone)
}

fn sleep(py: Python, seconds: f64, gil: bool) -> PyResult<PyNone> {
    let duration = std::time::Duration::from_micros((seconds * 1e6) as u64);
    if gil {
        std::thread::sleep(duration);
    } else {
        py.allow_threads(|| {
            std::thread::sleep(duration);
        });
    }
    Ok(PyNone)
}
