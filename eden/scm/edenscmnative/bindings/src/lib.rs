/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::PyModule;
use cpython::PyResult;
use cpython::Python;
use cpython_ext::PyNone;

mod functions;
mod modules;

/// Populate an existing empty module so it contains utilities.
pub fn populate_module(py: Python<'_>, module: &PyModule) -> PyResult<PyNone> {
    functions::populate_module(py, module)?;
    modules::populate_module(py, module)?;
    Ok(PyNone)
}
