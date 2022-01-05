/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::exc;
use cpython::FromPyObject;
use cpython::PyErr;
use cpython::PyObject;
use cpython::PyResult;
use cpython::Python;
use cpython::ToPyObject;

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Default, Hash, Ord)]
pub struct PyNone;

impl ToPyObject for PyNone {
    type ObjectType = PyObject;

    #[inline]
    fn to_py_object(&self, py: Python) -> PyObject {
        py.None()
    }
}

impl FromPyObject<'_> for PyNone {
    fn extract(py: Python, obj: &PyObject) -> PyResult<Self> {
        if *obj == py.None() {
            Ok(PyNone)
        } else {
            Err(PyErr::new::<exc::TypeError, _>(
                py,
                format!("Expected None but received {}", obj.get_type(py).name(py)),
            ))
        }
    }
}
