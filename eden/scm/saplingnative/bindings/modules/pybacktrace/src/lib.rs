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

    // By default, skip 1 wrapper frame
    // - pybacktrace::init_module::wrap
    m.add(py, "backtrace", py_fn!(py, backtrace_py(skip: usize = 1)))?;

    let info = &backtrace_python::SUPPORTED_INFO;
    let info_mod = PyModule::new(py, &format!("{name}.SUPPORTED_INFO"))?;
    info_mod.add(py, "os_arch", info.os_arch)?;
    info_mod.add(py, "c_evalframe", info.c_evalframe)?;
    info_mod.add(py, "is_supported", py_fn!(py, is_supported()))?;
    info_mod.add(py, "is_arch_translated", py_fn!(py, is_arch_translated()))?;

    m.add(py, "SUPPORTED_INFO", info_mod)?;
    m.add(py, "OFFSET_IP", backtrace_python::offsets::OFFSET_IP)?;
    m.add(
        py,
        "OFFSET_SP_FRAME",
        backtrace_python::offsets::OFFSET_SP_FRAME,
    )?;
    m.add(
        py,
        "OFFSET_SP_CODE",
        backtrace_python::offsets::OFFSET_SP_CODE,
    )?;
    m.add(
        py,
        "OFFSET_SP_LINE_NO",
        backtrace_python::offsets::OFFSET_SP_LINE_NO,
    )?;

    Ok(m)
}

/// Obtain Rust+Python backtrace for the current thread.
fn backtrace_py(_py: Python, mut skip: usize) -> PyResult<Vec<String>> {
    backtrace_python::init();
    let mut frames = Vec::with_capacity(32);
    backtrace_ext::trace_unsynchronized!(|frame: backtrace_ext::Frame| {
        if skip == 0 {
            let name = frame.resolve();
            frames.push(name);
        }
        skip = skip.saturating_sub(1);
        true
    });
    Ok(frames)
}

fn is_arch_translated(_py: Python) -> PyResult<bool> {
    Ok(backtrace_python::SUPPORTED_INFO.is_arch_translated())
}

fn is_supported(_py: Python) -> PyResult<bool> {
    Ok(backtrace_python::SUPPORTED_INFO.is_supported())
}
