/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use python3_sys as ffi;

extern "C" {
    fn PyInit_bdiff() -> *mut ffi::PyObject;
    fn PyInit_mpatch() -> *mut ffi::PyObject;
    fn PyInit_osutil() -> *mut ffi::PyObject;
    fn PyInit_parsers() -> *mut ffi::PyObject;
    fn PyInit_bser() -> *mut ffi::PyObject;
}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "cext"].join(".");
    let m = PyModule::new(py, &name)?;
    let (bdiff, mpatch, osutil, parsers, bser) = unsafe {
        (
            PyObject::from_borrowed_ptr(py, PyInit_bdiff()),
            PyObject::from_borrowed_ptr(py, PyInit_mpatch()),
            PyObject::from_borrowed_ptr(py, PyInit_osutil()),
            PyObject::from_borrowed_ptr(py, PyInit_parsers()),
            PyObject::from_borrowed_ptr(py, PyInit_bser()),
        )
    };
    m.add(py, "bdiff", bdiff)?;
    m.add(py, "mpatch", mpatch)?;
    m.add(py, "osutil", osutil)?;
    m.add(py, "parsers", parsers)?;
    m.add(py, "bser", bser)?;

    Ok(m)
}
