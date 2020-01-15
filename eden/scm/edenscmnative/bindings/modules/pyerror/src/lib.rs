/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::error;

py_exception!(error, IndexedLogError);
py_exception!(error, MetaLogError);
py_exception!(error, RustError);
py_exception!(error, RevisionstoreError);

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "error"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(py, "IndexedLogError", py.get_type::<IndexedLogError>())?;
    m.add(py, "MetaLogError", py.get_type::<MetaLogError>())?;
    m.add(py, "RustError", py.get_type::<RustError>())?;
    m.add(
        py,
        "RevisionstoreError",
        py.get_type::<RevisionstoreError>(),
    )?;

    register_error_handlers();

    Ok(m)
}

fn register_error_handlers() {
    fn specific_error_handler(py: Python, e: &error::Error) -> Option<PyErr> {
        if let Some(e) = e.downcast_ref::<indexedlog::Error>() {
            Some(PyErr::new::<IndexedLogError, _>(py, e.to_string()))
        } else if let Some(e) = e.downcast_ref::<metalog::Error>() {
            Some(PyErr::new::<MetaLogError, _>(py, e.to_string()))
        } else if let Some(e) = e.downcast_ref::<revisionstore::Error>() {
            Some(PyErr::new::<RevisionstoreError, _>(py, e.to_string()))
        } else {
            None
        }
    }

    fn fallback_error_handler(py: Python, e: &error::Error) -> Option<PyErr> {
        Some(PyErr::new::<RustError, _>(py, e.to_string()))
    }

    error::register("010-specific", specific_error_handler);
    error::register("999-fallback", fallback_error_handler);
}
