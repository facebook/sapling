/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Integrate cpython with anyhow

use anyhow::{Error, Result};
use cpython::{
    exc, py_exception, FromPyObject, ObjectProtocol, PyClone, PyList, PyModule, PyResult, Python,
};
use std::fmt;

/// Extends the `Result` type to allow conversion to `PyResult` from a native
/// Rust result.
///
/// If the error is created via [`AnyhowResultExt`], the original Python error
/// will be returned.
///
/// # Examples
///
/// ```
/// use anyhow::{format_err, Error};
/// use cpython::{exc, Python, PyResult};
/// use cpython_ext::error::ResultPyErrExt;
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
///    fail_if_negative(i).map_pyerr(py)
/// }
/// ```
pub trait ResultPyErrExt<T> {
    fn map_pyerr(self, py: Python<'_>) -> PyResult<T>;
}

/// Extends the `PyResult` type to allow conversion to `Result`.
///
/// # Examples
///
/// ```
/// use anyhow::Result;
/// use cpython_ext::error::{AnyhowResultExt, PyErr};
///
/// fn eval_py() -> Result<i32> {
///     let gil = cpython::Python::acquire_gil();
///     let py = gil.python();
///     let obj = py.eval("1 + 2", None, None).into_anyhow_result()?;
///     obj.extract(py).into_anyhow_result()
/// }
///
/// fn round_trip() -> cpython::PyResult<()> {
///     let gil = cpython::Python::acquire_gil();
///     let py = gil.python();
///     let res = py.eval("1 + 2", None, None).into_anyhow_result();
///     use cpython_ext::error::ResultPyErrExt;
///     res.map(|_| ()).map_pyerr(py)
/// }
/// ```
///
pub trait AnyhowResultExt<T> {
    fn into_anyhow_result(self) -> Result<T>;
}

py_exception!(error, PyIndexedLogError);
py_exception!(error, PyRustError);

impl<T, E: Into<Error>> ResultPyErrExt<T> for Result<T, E> {
    fn map_pyerr(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| {
            let e: anyhow::Error = e.into();
            let mut e = &e;
            loop {
                if let Some(e) = e.downcast_ref::<PyErr>() {
                    break e.inner.clone_ref(py);
                } else if let Some(inner) = e.downcast_ref::<anyhow::Error>() {
                    e = inner;
                } else if let Some(e) = e.downcast_ref::<indexedlog::Error>() {
                    break cpython::PyErr::new::<PyIndexedLogError, _>(py, e.to_string());
                } else if let Some(e) = e.downcast_ref::<std::io::Error>() {
                    break cpython::PyErr::new::<exc::IOError, _>(
                        py,
                        (e.raw_os_error(), e.to_string()),
                    );
                } else {
                    break cpython::PyErr::new::<PyRustError, _>(py, e.to_string());
                }
            }
        })
    }
}

impl<T> AnyhowResultExt<T> for PyResult<T> {
    fn into_anyhow_result(self) -> Result<T> {
        self.map_err(|e| Error::new(PyErr::from(e)))
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

pub fn format_py_error(py: Python, err: &cpython::PyErr) -> PyResult<String> {
    let traceback = PyModule::import(py, "traceback")?;
    let py_message = traceback.call(
        py,
        "format_exception",
        (&err.ptype, &err.pvalue, &err.ptraceback),
        None,
    )?;

    let py_lines = PyList::extract(py, &py_message)?;

    let lines: Vec<String> = py_lines
        .iter(py)
        .map(|l| l.extract::<String>(py).unwrap_or_default())
        .collect();

    Ok(lines.join(""))
}
