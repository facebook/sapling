/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "modules"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "get_bytecode", py_fn!(py, get_bytecode(name: &str)))?;
    m.add(py, "get_source", py_fn!(py, get_source(name: &str)))?;
    m.add(py, "list", py_fn!(py, list()))?;
    Ok(m)
}

fn get_bytecode(py: Python, name: &str) -> PyResult<Option<::pybytes::Bytes>> {
    match python_modules::find_module(name) {
        None => Ok(None),
        Some(m) => Ok(Some(to_bytes(py, m.byte_code())?)),
    }
}

fn get_source(py: Python, name: &str) -> PyResult<Option<::pybytes::Bytes>> {
    match python_modules::find_module(name) {
        None => Ok(None),
        Some(m) => Ok(Some(to_bytes(py, m.source_code().as_bytes())?)),
    }
}

fn list(_py: Python) -> PyResult<Vec<&'static str>> {
    Ok(python_modules::list_modules())
}

/// Converts to `pybytes::Bytes` without copying the slice.
fn to_bytes(py: Python, slice: &'static [u8]) -> PyResult<::pybytes::Bytes> {
    let bytes = minibytes::Bytes::from_static(slice);
    pybytes::Bytes::from_bytes(py, bytes)
}
