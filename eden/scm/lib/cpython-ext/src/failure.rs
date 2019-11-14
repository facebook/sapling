/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Integrate cpython with failure

use cpython::{ObjectProtocol, PyClone, PyResult, Python, PythonObjectWithTypeObject};
use failure::{AsFail, Error, Fallible};
use std::fmt;

/// Extends the `Result` type to allow conversion to `PyResult` by specifying a
/// Python Exception class to convert to.
///
/// If the error is created via [`FallibleExt`], the original Python error
/// will be returned.
///
/// # Examples
///
/// ```
/// use cpython::{exc, Python, PyResult};
/// use cpython_ext::failure::ResultPyErrExt;
/// use failure::{format_err, Error};
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

/// Extends the `PyResult` type to allow conversion to `Result`.
///
/// # Examples
///
/// ```
/// use cpython_ext::failure::{FallibleExt, PyErr};
/// use failure::Fallible;
///
/// fn eval_py() -> Fallible<i32> {
///     let gil = cpython::Python::acquire_gil();
///     let py = gil.python();
///     let obj = py.eval("1 + 2", None, None).into_fallible()?;
///     obj.extract(py).into_fallible()
/// }
///
/// fn round_trip() -> cpython::PyResult<()> {
///     let gil = cpython::Python::acquire_gil();
///     let py = gil.python();
///     let res = py.eval("1 + 2", None, None).into_fallible();
///     use cpython_ext::failure::ResultPyErrExt;
///     res.map(|_| ()).map_pyerr::<cpython::exc::ValueError>(py)
/// }
/// ```
///
pub trait FallibleExt<T> {
    fn into_fallible(self) -> Fallible<T>;
}

impl<T, E: AsFail> ResultPyErrExt<T> for Result<T, E> {
    fn map_pyerr<PE: PythonObjectWithTypeObject>(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| {
            let e = e.as_fail();
            if let Some(e) = e.downcast_ref::<PyErr>() {
                e.inner.clone_ref(py)
            } else {
                cpython::PyErr::new::<PE, _>(py, e.to_string())
            }
        })
    }
}

impl<T> FallibleExt<T> for PyResult<T> {
    fn into_fallible(self) -> Fallible<T> {
        self.map_err(|e| Error::from_boxed_compat(Box::new(PyErr::from(e))))
    }
}

/// An enhanced version of `PyErr` that implements [`std::error::Error`].
pub struct PyErr {
    inner: cpython::PyErr,
}

impl From<cpython::PyErr> for PyErr {
    fn from(e: cpython::PyErr) -> PyErr {
        PyErr { inner: e }
    }
}

impl From<PyErr> for cpython::PyErr {
    fn from(e: PyErr) -> cpython::PyErr {
        e.inner
    }
}

impl fmt::Display for PyErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let repr = self
            .inner
            .pvalue
            .as_ref()
            .unwrap_or_else(|| &self.inner.ptype)
            .repr(py)
            .map(|s| s.to_string_lossy(py).to_string())
            .unwrap_or_else(|_| "<error in repr>".into());
        write!(f, "{}", repr)
    }
}

impl fmt::Debug for PyErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl std::error::Error for PyErr {}

impl serde::ser::Error for PyErr {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let err = cpython::PyErr::new::<cpython::exc::TypeError, _>(py, msg.to_string());
        Self { inner: err }
    }
}

impl PyErr {
    pub fn into_inner(self) -> cpython::PyErr {
        self.inner
    }
}
