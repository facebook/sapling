/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::convert::Serde;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "procinfo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "process_ancestors",
        py_fn!(py, process_ancestors(max_depth: usize)),
    )?;
    Ok(m)
}

/// Walk the parent process tree up to `max_depth` levels.
/// Returns a list of dicts with "name" and "pid" keys.
fn process_ancestors(_py: Python, max_depth: usize) -> PyResult<Serde<Vec<procinfo::ProcInfo>>> {
    let ancestors = procinfo::process_ancestors(max_depth);
    Ok(Serde(ancestors))
}
