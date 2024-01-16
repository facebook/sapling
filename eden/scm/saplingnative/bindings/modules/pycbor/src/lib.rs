/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "cbor"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "loads", py_fn!(py, parse(bytes: PyBytes)))?;
    m.add(
        py,
        "dumps",
        py_fn!(py, unparse(value: Serde<serde_cbor::Value>, packed: bool = false)),
    )?;
    Ok(m)
}

fn parse(py: Python, bytes: PyBytes) -> PyResult<Serde<serde_cbor::Value>> {
    let data = bytes.data(py);
    let value = serde_cbor::from_slice(data).map_pyerr(py)?;
    Ok(Serde(value))
}

fn unparse(py: Python, value: Serde<serde_cbor::Value>, packed: bool) -> PyResult<PyBytes> {
    let bytes: Vec<u8> = if packed {
        serde_cbor::ser::to_vec_packed(&value.0).map_pyerr(py)?
    } else {
        serde_cbor::to_vec(&value.0).map_pyerr(py)?
    };
    Ok(PyBytes::new(py, &bytes))
}
