// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use std::io;

use cpython::*;
use cpython_ext::SimplePyBuf;

use zstd::stream::{decode_all, encode_all};
use zstdelta::{apply, diff};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "zstd"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "apply",
        py_fn!(py, apply_py(base: &PyObject, delta: &PyObject)),
    )?;
    m.add(
        py,
        "diff",
        py_fn!(py, diff_py(base: &PyObject, data: &PyObject)),
    )?;
    m.add(py, "decode_all", py_fn!(py, decode_all_py(data: &PyObject)))?;
    m.add(
        py,
        "encode_all",
        py_fn!(py, encode_all_py(data: &PyObject, level: i32)),
    )?;
    Ok(m)
}

/// Convert `io::Result<Vec<u8>>` to a `PyResult<PyBytes>`.
fn convert(py: Python, result: io::Result<Vec<u8>>) -> PyResult<PyBytes> {
    result
        .map_err(|e| PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))
        .map(|buf| PyBytes::new(py, &buf))
}

fn diff_py(py: Python, base: &PyObject, data: &PyObject) -> PyResult<PyBytes> {
    let base = SimplePyBuf::new(py, base);
    let data = SimplePyBuf::new(py, data);
    convert(py, diff(base.as_ref(), data.as_ref()))
}

fn apply_py(py: Python, base: &PyObject, delta: &PyObject) -> PyResult<PyBytes> {
    let base = SimplePyBuf::new(py, base);
    let delta = SimplePyBuf::new(py, delta);
    convert(py, apply(base.as_ref(), delta.as_ref()))
}

fn decode_all_py(py: Python, data: &PyObject) -> PyResult<PyBytes> {
    let data = SimplePyBuf::new(py, data);
    convert(py, decode_all(io::Cursor::new(data.as_ref())))
}

fn encode_all_py(py: Python, data: &PyObject, level: i32) -> PyResult<PyBytes> {
    let data = SimplePyBuf::new(py, data);
    convert(py, encode_all(io::Cursor::new(data.as_ref()), level))
}
