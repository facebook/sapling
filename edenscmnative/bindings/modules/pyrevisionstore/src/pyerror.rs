// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{PyErr, Python};
use failure::{Error, Fail};

#[derive(Debug, Fail)]
#[fail(display = "Python Error: {:?}", _0)]
pub struct PythonError(PyErr);

impl From<PyErr> for PythonError {
    fn from(err: PyErr) -> Self {
        PythonError(err)
    }
}

pub fn pyerr_to_error(_py: Python, py_err: PyErr) -> Error {
    PythonError::from(py_err).into()
}
