// Copyright 2018 Facebook, Inc.

use cpython::{exc, PyErr, PyResult, Python};
use failure::AsFail;

pub trait AsPyErr {
    fn as_pyerr(&self, py: Python) -> PyErr;
}

impl<T: AsFail> AsPyErr for T {
    fn as_pyerr(&self, py: Python) -> PyErr {
        PyErr::new::<exc::RuntimeError, _>(py, format!("{}", self.as_fail()))
    }
}

pub trait IntoPyResult<T, E> {
    fn into_pyresult(self, py: Python) -> PyResult<T>;
}

impl<T, E: AsFail> IntoPyResult<T, E> for Result<T, E> {
    fn into_pyresult(self, py: Python) -> PyResult<T> {
        self.map_err(|e| e.as_pyerr(py))
    }
}
