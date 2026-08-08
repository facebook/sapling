/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "fs"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "canonicalize", py_fn!(py, canonicalize(path: &str)))?;
    m.add(py, "fstype", py_fn!(py, fstype(path: &PyPath)))?;
    m.add(py, "short_name", py_fn!(py, short_name(path: &PyPath)))?;
    m.add(
        py,
        "remove_short_name",
        py_fn!(py, remove_short_name(path: &PyPath)),
    )?;
    Ok(m)
}

fn canonicalize(py: Python, path: &str) -> PyResult<String> {
    let canonical_path = std::fs::canonicalize(path).map_pyerr(py)?;
    match canonical_path.to_str() {
        Some(s) => Ok(s.to_string()),
        None => {
            let message = format!("canonicalized({path}) is non-utf-8");
            Err(PyErr::new::<exc::ValueError, _>(py, message))
        }
    }
}

fn fstype(py: Python, path: &PyPath) -> PyResult<String> {
    let fstype = fsinfo::fstype(path).map_pyerr(py)?;
    Ok(fstype.to_string())
}

fn short_name(py: Python, path: &PyPath) -> PyResult<Option<String>> {
    Ok(win32_8dot3::short_name_at(path.as_path())
        .map_pyerr(py)?
        .map(|name| name.to_string_lossy().into_owned()))
}

fn remove_short_name(py: Python, path: &PyPath) -> PyResult<PyNone> {
    win32_8dot3::remove_short_name_at(path.as_path()).map_pyerr(py)?;
    Ok(PyNone)
}
