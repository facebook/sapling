/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::ResultPyErrExt;
use pprint::pformat_value;
use pprint::Value;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pprint"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "pformat", py_fn!(py, pformat(obj: PyObject)))?;
    Ok(m)
}

fn pformat(py: Python, obj: PyObject) -> PyResult<String> {
    let value: Value = cpython_ext::de::from_object(py, obj).map_pyerr(py)?;
    Ok(pformat_value(&value))
}
