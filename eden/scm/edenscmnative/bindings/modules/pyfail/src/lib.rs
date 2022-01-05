/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use cpython::*;
use cpython_ext::PyNone;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "fail"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "failpoint", py_fn!(py, failpoint(name: &str)))?;
    Ok(m)
}

fn failpoint(py: Python, name: &str) -> PyResult<PyNone> {
    if let Some(e) = fail::eval(name, |_| fail_error(name)) {
        Err(cpython_ext::error::translate_io_error(py, &e).into())
    } else {
        Ok(PyNone)
    }
}

fn fail_error(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Other,
        format!("failpoint '{}' set by FAILPOINTS", name),
    )
}
