// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::ptr;

use python27_sys as ffi;

pub fn pyset_new(py: cpython::Python<'_>) -> cpython::PyResult<cpython::PyObject> {
    unsafe {
        let inner = ffi::PySet_New(ptr::null_mut());
        if inner.is_null() {
            return Err(cpython::PyErr::new::<cpython::exc::RuntimeError, _>(
                py,
                "Could not allocate set",
            ));
        }
        Ok(cpython::PyObject::from_owned_ptr(py, inner))
    }
}

pub fn pyset_add<V: cpython::ToPyObject>(
    py: cpython::Python<'_>,
    set: &mut cpython::PyObject,
    value: V,
) -> cpython::PyResult<()> {
    value.with_borrowed_ptr(py, |value| unsafe {
        let result = ffi::PySet_Add(set.as_ptr(), value);
        if result != -1 {
            Ok(())
        } else {
            Err(cpython::PyErr::fetch(py))
        }
    })
}
