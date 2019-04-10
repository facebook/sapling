// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{exc, PyErr, Python, PythonObjectWithTypeObject};
use failure::{Error, Fail};

use revisionstore::error::KeyError;

#[derive(Debug, Fail)]
#[fail(display = "Python Error: {:?}", _0)]
pub struct PythonError(PyErr);

impl From<PyErr> for PythonError {
    fn from(err: PyErr) -> Self {
        PythonError(err)
    }
}

pub fn pyerr_to_error(py: Python, py_err: PyErr) -> Error {
    let err_type = py_err.get_type(py);

    let py_err: Error = PythonError::from(py_err).into();
    if err_type == exc::KeyError::type_object(py) {
        KeyError::new(py_err).into()
    } else {
        // Unknown python error; return it as is.
        py_err
    }
}
