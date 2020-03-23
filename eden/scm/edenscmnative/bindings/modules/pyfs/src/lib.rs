/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::{PyPath, ResultPyErrExt, Str};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "fs"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "fstype", py_fn!(py, fstype(path: &PyPath)))?;
    Ok(m)
}

fn fstype(py: Python, path: &PyPath) -> PyResult<Str> {
    let fstype = fsinfo::fstype(path).map_pyerr(py)?;
    Ok(fstype.to_string().into())
}
