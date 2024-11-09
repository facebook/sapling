/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ptr;

use python3_sys as ffi;

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
