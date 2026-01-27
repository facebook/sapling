/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "util"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "username", py_fn!(py, username()))?;
    Ok(m)
}

fn username(py: Python) -> PyResult<String> {
    sysutil::username().map_pyerr(py)
}
