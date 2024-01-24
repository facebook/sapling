/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module exposes Rust's progress rendering to Python.

use cpython::*;
use cpython_ext::PyNone;
use progress_model::Registry;

pub(crate) fn simple(_py: Python) -> PyResult<String> {
    // XXX: Default config is hard-coded. This API is for debugging purpose.
    // Formal progress rendering is done by pure Rust.
    let config = Default::default();
    let reg = Registry::main();
    Ok(progress_render::simple::render_string(reg, &config))
}

pub(crate) fn debug(_py: Python) -> PyResult<String> {
    let reg = Registry::main();
    Ok(format!("{:?}", reg))
}

pub(crate) fn step(_py: Python) -> PyResult<PyNone> {
    Registry::main().step();
    Ok(PyNone)
}
