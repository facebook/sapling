/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::{allocate_pybytes, vec_to_pyobj, SimplePyBuf};
use cpython_failure::ResultPyErrExt;
use lz4_pyframe::{compress, compresshc, decompress_into, decompress_size};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "lz4"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "compress", py_fn!(py, compress_py(data: PyObject)))?;
    m.add(py, "compresshc", py_fn!(py, compresshc_py(data: PyObject)))?;
    m.add(py, "decompress", py_fn!(py, decompress_py(data: PyObject)))?;
    Ok(m)
}

fn compress_py(py: Python, data: PyObject) -> PyResult<PyObject> {
    let data = SimplePyBuf::new(py, &data);
    compress(data.as_ref())
        .map_pyerr::<exc::RuntimeError>(py)
        .map(|bytes| vec_to_pyobj(py, bytes))
}

fn compresshc_py(py: Python, data: PyObject) -> PyResult<PyObject> {
    let data = SimplePyBuf::new(py, &data);
    compresshc(data.as_ref())
        .map_pyerr::<exc::RuntimeError>(py)
        .map(|bytes| vec_to_pyobj(py, bytes))
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
