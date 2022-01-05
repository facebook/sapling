/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::io;

use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::SimplePyBuf;
use zstd::stream::decode_all;
use zstd::stream::encode_all;
use zstd::stream::raw::Decoder;
use zstd::stream::raw::InBuffer;
use zstd::stream::raw::Operation;
use zstd::stream::raw::OutBuffer;
use zstdelta::apply;
use zstdelta::diff;

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
    m.add_class::<zstream>(py)?;
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

py_class!(pub class zstream |py| {
    data decoder: RefCell<Decoder<'static>>;

    def __new__(_cls) -> PyResult<zstream> {
        let decoder = Decoder::new().map_pyerr(py)?;

        zstream::create_instance(py, RefCell::new(decoder))
    }

    def decompress_buffer(&self, data: &PyObject) -> PyResult<PyBytes> {
        let data = SimplePyBuf::new(py, data);
        let mut decoder = self.decoder(py).borrow_mut();
        let mut src = InBuffer::around(data.as_ref());
        let mut dst = vec![0u8; zstd_safe::dstream_out_size()];
        let mut dst = OutBuffer::around(&mut dst);

        while src.pos < src.src.len() {
            decoder.run(&mut src, &mut dst).map_pyerr(py)?;
        }
        Ok(PyBytes::new(py, dst.as_slice()))
    }
});
