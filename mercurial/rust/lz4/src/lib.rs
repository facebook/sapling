// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

#[macro_use]
extern crate cpython;
extern crate cpython_failure;
extern crate lz4_pyframe;
extern crate python27_sys;

// FIXME: Share pybuf implementation across libraries.
mod pybuf;

use cpython::{exc, PyBytes, PyObject, PyResult, Python};
use cpython_failure::ResultPyErrExt;
use lz4_pyframe::{compress, decompress};
use pybuf::SimplePyBuf;

py_module_initializer!(lz4, initlz4, PyInit_lz4, |py, m| {
    m.add(py, "compress", py_fn!(py, compress_py(data: PyObject)))?;
    m.add(py, "decompress", py_fn!(py, decompress_py(data: PyObject)))?;
    Ok(())
});

fn compress_py(py: Python, data: PyObject) -> PyResult<PyBytes> {
    let data = SimplePyBuf::new(py, &data);
    compress(data.as_ref())
        .map_pyerr::<exc::RuntimeError>(py)
        .map(|bytes| PyBytes::new(py, bytes.as_ref()))
}

fn decompress_py(py: Python, data: PyObject) -> PyResult<PyBytes> {
    let data = SimplePyBuf::new(py, &data);
    decompress(data.as_ref())
        .map_pyerr::<exc::RuntimeError>(py)
        .map(|bytes| PyBytes::new(py, bytes.as_ref()))
}
