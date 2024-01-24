/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;

use cpython::*;
use cpython_ext::convert::Serde;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "xdiff"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "blocks", py_fn!(py, blocks(a: PyObject, b: PyObject)))?;
    m.add(
        py,
        "edit_cost",
        py_fn!(py, edit_cost(a: PyObject, b: PyObject, max_edit_cost: u64)),
    )?;
    Ok(m)
}

// (a: bytes | str, b: bytes | str) -> List[(a1, a2, b1, b2)].
// Yield matching blocks.
fn blocks(py: Python, a: PyObject, b: PyObject) -> PyResult<Serde<Vec<(u64, u64, u64, u64)>>> {
    let a = a.extract::<BytesOrStr>(py)?;
    let b = b.extract::<BytesOrStr>(py)?;
    let a_data = a.as_bytes(py)?;
    let b_data = b.as_bytes(py)?;
    let result = py.allow_threads(|| xdiff::blocks(&a_data, &b_data));
    Ok(Serde(result))
}

// (a: bytes | str, b: bytes | str, max_edit_cost) -> edit_cost
fn edit_cost(py: Python, a: PyObject, b: PyObject, max_edit_cost: u64) -> PyResult<u64> {
    let a = a.extract::<BytesOrStr>(py)?;
    let b = b.extract::<BytesOrStr>(py)?;
    let a_data = a.as_bytes(py)?;
    let b_data = b.as_bytes(py)?;
    let result = py.allow_threads(|| xdiff::edit_cost(&a_data, &b_data, max_edit_cost));
    Ok(result)
}

enum BytesOrStr {
    Bytes(PyBytes),
    Str(PyString),
}

impl<'s> FromPyObject<'s> for BytesOrStr {
    fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
        if let Ok(bytes_obj) = obj.extract::<PyBytes>(py) {
            Ok(Self::Bytes(bytes_obj))
        } else {
            let str_obj = obj.extract::<PyString>(py)?;
            Ok(Self::Str(str_obj))
        }
    }
}

impl BytesOrStr {
    fn as_bytes<'a>(&'a self, py: Python) -> PyResult<Cow<'a, [u8]>> {
        match self {
            Self::Bytes(obj) => Ok(Cow::Borrowed(obj.data(py))),
            Self::Str(obj) => {
                let data = obj.data(py).to_string(py)?;
                let data = match data {
                    Cow::Borrowed(data) => Cow::Borrowed(data.as_bytes()),
                    Cow::Owned(data) => Cow::Owned(data.into_bytes()),
                };
                Ok(data)
            }
        }
    }
}
