/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::PyErr;
use failure::Fail;

#[derive(Debug, Fail)]
#[fail(display = "Python Error: {:?}", _0)]
pub struct PythonError(PyErr);

impl From<PyErr> for PythonError {
    fn from(err: PyErr) -> Self {
        PythonError(err)
    }
}
