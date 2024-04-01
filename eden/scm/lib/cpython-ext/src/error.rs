/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Integrate cpython with anyhow

use std::collections::BTreeMap;
use std::fmt;

pub use anyhow::Error;
pub use anyhow::Result;
use cpython::exc;
use cpython::FromPyObject;
use cpython::GILGuard;
use cpython::ObjectProtocol;
use cpython::PyClone;
use cpython::PyList;
use cpython::PyModule;
use cpython::PyResult;
use cpython::Python;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Extends the `Result` type to allow conversion to `PyResult` from a native
/// Rust result.
///
/// If the error is created via [`AnyhowResultExt`], the original Python error
/// will be returned.
///
/// # Examples
///
/// ```
/// use anyhow::format_err;
/// use anyhow::Error;
/// use cpython::exc;
/// use cpython::PyResult;
/// use cpython::Python;
/// use cpython_ext::error::ResultPyErrExt;
///
/// fn fail_if_negative(i: i32) -> Result<i32, Error> {
///     if (i >= 0) {
///         Ok(i)
///     } else {
///         Err(format_err!("{} is negative", i))
///     }
/// }
///
/// fn py_fail_if_negative(py: Python, i: i32) -> PyResult<i32> {
///     fail_if_negative(i).map_pyerr(py)
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
/// use cpython_ext::error::AnyhowResultExt;
/// use cpython_ext::error::PyErr;
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
pub trait AnyhowResultExt<T> {
    fn into_anyhow_result(self) -> Result<T>;
}

pub type AnyhowErrorIntoPyErrFunc = fn(Python, &anyhow::Error) -> Option<cpython::PyErr>;

static INTO_PYERR_FUNC_LIST: Lazy<Mutex<BTreeMap<&'static str, AnyhowErrorIntoPyErrFunc>>> =
    Lazy::new(|| Default::default());

/// Register a function to convert [`anyhow::Error`] to [`PyErr`].
/// For multiple functions, those with smaller name are executed first.
/// Registering a function with an existing name will override that function.
///
/// This affects users of `map_pyerr`.
pub fn register(name: &'static str, func: AnyhowErrorIntoPyErrFunc) {
    let mut list = INTO_PYERR_FUNC_LIST.lock();
    list.insert(name, func);
}

impl<T, E: Into<Error>> ResultPyErrExt<T> for Result<T, E> {
    fn map_pyerr(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| {
            let e: anyhow::Error = e.into();

            if let Some(e) = e.downcast_ref::<PyErr>() {
                return e.inner.clone_ref(py);
            }

            for func in INTO_PYERR_FUNC_LIST.lock().values() {
                if let Some(err) = (func)(py, &e) {
                    return err;
                }
            }

            // Nothing matches. Fallback to RuntimeError.
            // Hopefully this is not really used.
            return cpython::PyErr::new::<exc::RuntimeError, _>(py, format!("{:?}", e));
        })
    }
}

pub fn translate_io_error(py: Python, e: &std::io::Error) -> cpython::PyErr {
    let errno = {
        // Attempt to infer the errno from error type.
        //
        // This is useful for Python to test error type for
        // io::Error generated by non-syscalls.  For
        // example, pipe uses userspace pipe implementation
        // and can return BrokenPipe without errno.
        //
        // See rust's decode_error_kind for the reversed
        // direction.
        use std::io::ErrorKind::*;
        match e.kind() {
            NotFound => Some(libc::ENOENT),
            PermissionDenied => Some(libc::EPERM),
            ConnectionRefused => Some(libc::ECONNREFUSED),
            ConnectionReset => Some(libc::ECONNRESET),
            ConnectionAborted => Some(libc::ECONNABORTED),
            NotConnected => Some(libc::ENOTCONN),
            AddrInUse => Some(libc::EADDRINUSE),
            AddrNotAvailable => Some(libc::EADDRNOTAVAIL),
            BrokenPipe => Some(libc::EPIPE),
            AlreadyExists => Some(libc::EEXIST),
            WouldBlock => Some(libc::EWOULDBLOCK),
            InvalidInput => Some(libc::EINVAL),
            TimedOut => Some(libc::ETIMEDOUT),
            Interrupted => Some(libc::EINTR),
            // No clues about the errno based on kind. Expose the raw OS error code.
            _ => e.raw_os_error(),
        }
    };
    cpython::PyErr::new::<exc::IOError, _>(py, (errno, e.to_string()))
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

impl PyErr {
    pub fn clone(&self, py: Python) -> Self {
        self.inner.clone_ref(py).into()
    }
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
        // To read the python error info, we need the gil. It's not safe to blindly acquire the
        // gil, since it may be held by another thread and we could deadlock if other Rust locks
        // are taken and the lock ordering is inconsistent. So let's only produce a meaningful
        // message if we have the gil already.
        // This mostly affects tracing collectors, since they may choose to format the error at any
        // point.
        if GILGuard::check() {
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
            write!(f, "{}", repr)?;
            if std::env::var("RUST_BACKTRACE").is_ok() {
                if let Ok(s) = format_py_error(py, &self.inner) {
                    write!(f, "\n{}", s)?;
                }
            }
        } else {
            write!(f, "<unable to print PyErr - GIL is not held>")?;
        }
        Ok(())
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
        // We should generally not be acquiring the gil here, since we may encounter lock-order
        // races with other Rust locks. But in this case we know serde logic is being invoked from
        // the cpython-ext Serializer and Deserializer classes, which ensure the GIL is held
        // already.
        let gil = Python::acquire_gil();
        let py = gil.python();
        let err = cpython::PyErr::new::<cpython::exc::TypeError, _>(py, msg.to_string());
        Self { inner: err }
    }
}

impl serde::de::Error for PyErr {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        // See comment in ser implementation above for info about acquiring the gil here.
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

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn test_dont_lose_anyhow_context() {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let err: Result<()> = Err(anyhow::anyhow!("first")).context("second");

        assert_eq!(
            format!("{}", err.map_pyerr(py).unwrap_err().pvalue.unwrap()),
            "second\n\nCaused by:\n    first"
        );
    }
}