// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//! Integrate cpython with failure

extern crate cpython;
extern crate failure;

use cpython::{PyErr, PyResult, Python, PythonObjectWithTypeObject};
use failure::AsFail;

/// Extends the `Result` type to allow conversion to `PyResult` by specifying a
/// Python Exception class to convert to.
///
/// # Examples
///
/// ```
/// extern crate cpython;
/// extern crate cpython_failure;
/// #[macro_use] extern crate failure;
///
/// use cpython::{exc, Python, PyResult};
/// use cpython_failure::ResultPyErrExt;
/// use failure::Error;
///
/// fn fail_if_negative(i: i32) -> Result<i32, Error> {
///    if (i >= 0) {
///       Ok(i)
///    } else {
///       Err(format_err!("{} is negative", i))
///    }
/// }
///
/// fn py_fail_if_negative(py: Python, i: i32) -> PyResult<i32> {
///    fail_if_negative(i).map_pyerr::<exc::ValueError>(py)
/// }
/// ```
pub trait ResultPyErrExt<T> {
    fn map_pyerr<PE: PythonObjectWithTypeObject>(self, py: Python<'_>) -> PyResult<T>;
}

impl<T, E: AsFail> ResultPyErrExt<T> for Result<T, E> {
    fn map_pyerr<PE: PythonObjectWithTypeObject>(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| PyErr::new::<PE, _>(py, e.as_fail().to_string()))
    }
}
