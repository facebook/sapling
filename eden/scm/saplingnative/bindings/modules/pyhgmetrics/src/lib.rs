/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "hgmetrics"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "incrementcounter",
        py_fn!(py, increment_counter(key: &str, value: usize = 1)),
    )?;
    m.add(py, "summarize", py_fn!(py, summarize()))?;

    m.add(
        py,
        "samplingcategory",
        py_fn!(py, sampling_category(key: &str)),
    )?;
    m.add(
        py,
        "appendsamples",
        py_fn!(py, append_samples(data: String)),
    )?;

    Ok(m)
}

fn increment_counter(_py: Python, key: &str, value: usize) -> PyResult<PyNone> {
    hg_metrics::increment_counter(key, value);
    Ok(PyNone)
}

fn summarize(_py: Python) -> PyResult<HashMap<String, usize>> {
    Ok(hg_metrics::summarize())
}

fn sampling_category(_py: Python, key: &str) -> PyResult<Option<String>> {
    Ok(match sampling::CONFIG.get() {
        Some(Some(sc)) => sc.category(key).map(|cat| cat.to_string()),
        _ => None,
    })
}

fn append_samples(py: Python, data: String) -> PyResult<PyNone> {
    if let Some(Some(sc)) = sampling::CONFIG.get() {
        let mut file = sc.file();
        file.write_all(data.as_bytes()).map_pyerr(py)?;
        file.write_all(b"\0").map_pyerr(py)?;
    }
    Ok(PyNone)
}
