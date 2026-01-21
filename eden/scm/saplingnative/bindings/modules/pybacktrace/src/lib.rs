/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "backtrace"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "backtrace", py_fn!(py, backtrace_py()))?;

    let info = &backtrace_python::SUPPORTED_INFO;
    let info_mod = PyModule::new(py, &format!("{name}.SUPPORTED_INFO"))?;
    info_mod.add(py, "os_arch", info.os_arch)?;
    info_mod.add(py, "c_evalframe", info.c_evalframe)?;

    m.add(py, "SUPPORTED_INFO", info_mod)?;

    Ok(m)
}

/// Obtain Rust+Python backtrace for the current thread.
fn backtrace_py(_py: Python) -> PyResult<Vec<String>> {
    backtrace_python::init();
    let mut frames = Vec::with_capacity(32);
    backtrace_ext::trace_unsynchronized!(|frame: backtrace_ext::Frame| {
        let name = frame.resolve();
        frames.push(name);
        true
    });
    Ok(frames)
}
