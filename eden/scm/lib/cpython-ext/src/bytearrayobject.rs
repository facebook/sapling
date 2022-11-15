/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::mem;

use cpython::PyObject as RustPyObject;
use cpython::Python as RustPythonGILGuard;
use ffi::PyByteArray_Size;
use ffi::PyObject;
use ffi::PyVarObject;
use ffi::Py_ssize_t;
use python3_sys as ffi;

// Ideally this struct comes from bindgen to ensure it's up-to-date
// with what the current cpython has. However that significantly
// complicates the build step.
#[repr(C)]
struct PyByteArrayObject {
    pub ob_base: PyVarObject,
    pub ob_alloc: Py_ssize_t,
    pub ob_bytes: *mut u8,
    pub ob_start: *mut u8,
    // These fields exist in cpython but we don't care about them.
    // pub ob_exports: c_int | Py_ssize_t,
}

/// Consume a `Vec<u8>`. Create a Python `bytearray` object.
/// Bytes stored are not copied.
pub fn vec_to_pyobj(py: RustPythonGILGuard<'_>, mut value: Vec<u8>) -> RustPyObject {
    let capacity = value.capacity();
    let obj = unsafe { pybytearray_from_slice(py, value.as_mut(), Some(capacity)) };
    // now cpython is responsible to free `value`.
    mem::forget(value);
    obj
}

/// Consume a `Box<[u8]>`. Create a Python `bytearray` object.
/// Bytes stored are not copied.
pub fn boxed_slice_to_pyobj(py: RustPythonGILGuard<'_>, mut value: Box<[u8]>) -> RustPyObject {
    let obj = unsafe { pybytearray_from_slice(py, value.as_mut(), None) };
    // now cpython is responsible to free `value`.
    mem::forget(value);
    obj
}

/// Similar to `PyByteArray_FromStringAndSize(value)` without copying `value`.
///
/// CPython will free `value` when releasing the python object. When `value` is
/// a prefix of a larger buffer, set `alloc_size` so CPython can free the buffer
/// properly.
///
/// To avoid double-free from both CPython and Rust, the callsite must
/// `mem::forget` the owner of `value` after this function.
unsafe fn pybytearray_from_slice(
    py: RustPythonGILGuard,
    value: &mut [u8],
    alloc_size: Option<usize>,
) -> RustPyObject {
    let ptr: *mut PyObject = ffi::PyByteArray_FromStringAndSize(std::ptr::null(), 0);
    let typed: *mut PyByteArrayObject = mem::transmute(ptr);

    // We want to rewrite ob_alloc and ob_bytes directly. Ensure that
    // they are not allocated by cpython.
    assert_eq!(
        (*typed).ob_alloc,
        0,
        "PyByteArray_FromStringAndSize(size=0) should not alloc"
    );
    assert!(
        (*typed).ob_bytes.is_null(),
        "PyByteArray_FromStringAndSize(size=0) should not alloc"
    );

    // There are no public cpython APIs to update these fields.
    // So we update them directly based on our understanding of
    // the PyByteArrayObject struct.
    (*typed).ob_alloc = alloc_size.unwrap_or_else(|| value.len()) as Py_ssize_t;
    (*typed).ob_bytes = value.as_mut_ptr();
    {
        (*typed).ob_base.ob_size = value.len() as Py_ssize_t;
        (*typed).ob_start = (*typed).ob_bytes;
    }

    // Sanity check: cpython's understanding of the bytes matches ours.
    let ptr: *mut PyObject = typed as *mut PyObject;
    assert_eq!(
        PyByteArray_Size(ptr) as usize,
        value.len(),
        "PyByteArray struct mismatch (ob_size)"
    );
    assert_eq!(
        ffi::PyByteArray_AsString(ptr) as *const u8,
        value.as_ptr(),
        "PyByteArray struct mismatch (ob_bytes)"
    );

    RustPyObject::from_owned_ptr(py, ptr)
}
