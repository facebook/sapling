/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "fs"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "canonicalize", py_fn!(py, canonicalize(path: &str)))?;
    m.add(py, "fstype", py_fn!(py, fstype(path: &PyPath)))?;
    Ok(m)
}

fn canonicalize(py: Python, path: &str) -> PyResult<String> {
    let canonical_path = std::fs::canonicalize(path).map_pyerr(py)?;
    match canonical_path.to_str() {
        Some(s) => Ok(s.to_string()),
        None => {
            let message = format!("canonicalized({}) is non-utf-8", path);
            Err(PyErr::new::<exc::ValueError, _>(py, message))
        }
    }
}

fn fstype(py: Python, path: &PyPath) -> PyResult<Str> {
    let fstype = fsinfo::fstype(path).map_pyerr(py)?;
    Ok(fstype.to_string().into())
}
