// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

#[macro_use]
extern crate cpython;
extern crate cpython_ext;
extern crate cpython_failure;
extern crate lz4_pyframe;
extern crate python27_sys;

use cpython::{exc, PyObject, PyResult, Python};
use cpython_ext::{allocate_pybytes, boxed_slice_to_pyobj, SimplePyBuf};
use cpython_failure::ResultPyErrExt;
use lz4_pyframe::{compress, decompress_into, decompress_size};

py_module_initializer!(lz4, initlz4, PyInit_lz4, |py, m| {
    m.add(py, "compress", py_fn!(py, compress_py(data: PyObject)))?;
    m.add(py, "decompress", py_fn!(py, decompress_py(data: PyObject)))?;
    Ok(())
});

fn compress_py(py: Python, data: PyObject) -> PyResult<PyObject> {
    let data = SimplePyBuf::new(py, &data);
    compress(data.as_ref())
        .map_pyerr::<exc::RuntimeError>(py)
        .map(|bytes| boxed_slice_to_pyobj(py, bytes))
}

fn decompress_py(py: Python, data: PyObject) -> PyResult<PyObject> {
    let data = SimplePyBuf::new(py, &data);
    let data = data.as_ref();
    let size = decompress_size(data).map_pyerr::<exc::RuntimeError>(py)?;
    let (obj, slice) = allocate_pybytes(py, size);
    decompress_into(data, slice)
        .map_pyerr::<exc::RuntimeError>(py)
        .map(move |_| obj)
}
