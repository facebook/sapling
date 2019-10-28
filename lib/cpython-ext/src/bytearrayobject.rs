// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{PyObject as RustPyObject, Python as RustPythonGILGuard};
#[cfg(feature = "python2")]
use python27_sys::{
    PyByteArray_Size, PyByteArray_Type, PyObject, PyTypeObject, Py_ssize_t, _PyObject_New,
};
#[cfg(feature = "python3")]
use python3_sys::{
    PyByteArray_Size, PyByteArray_Type, PyObject, PyTypeObject, Py_ssize_t, _PyObject_New,
};
use std::mem;
use std::os::raw::c_int;

// From Python bytearrayobject.h. Must match the C definition.
#[repr(C)]
struct PyByteArrayObject {
    #[cfg(py_sys_config = "Py_TRACE_REFS")]
    pub _ob_next: *mut PyObject,
    #[cfg(py_sys_config = "Py_TRACE_REFS")]
    pub _ob_prev: *mut PyObject,
    pub ob_refcnt: Py_ssize_t,
    pub ob_type: *mut PyTypeObject,
    pub ob_size: Py_ssize_t,
    pub ob_exports: c_int,
    pub ob_alloc: Py_ssize_t,
    pub ob_bytes: *mut u8,
}

/// Consume a `Vec<u8>`. Create a Python `bytearray` object.
/// Bytes stored are not copied.
pub fn vec_to_pyobj(py: RustPythonGILGuard<'_>, mut value: Vec<u8>) -> RustPyObject {
    unsafe {
        let ptr: *mut PyObject = _PyObject_New(&mut PyByteArray_Type as *mut PyTypeObject);
        let typed: *mut PyByteArrayObject = mem::transmute(ptr);
        (*typed).ob_size = value.len() as Py_ssize_t;
        (*typed).ob_exports = 0;
        (*typed).ob_alloc = value.capacity() as Py_ssize_t;
        (*typed).ob_bytes = value.as_mut_ptr();
        assert_eq!(
            PyByteArray_Size(ptr) as usize,
            value.len(),
            "PyByteArray struct mismatch"
        );
        mem::forget(value);
        RustPyObject::from_owned_ptr(py, ptr)
    }
}

/// Consume a `Box<[u8]>`. Create a Python `bytearray` object.
/// Bytes stored are not copied.
pub fn boxed_slice_to_pyobj(py: RustPythonGILGuard<'_>, mut value: Box<[u8]>) -> RustPyObject {
    unsafe {
        let ptr: *mut PyObject = _PyObject_New(&mut PyByteArray_Type as *mut PyTypeObject);
        let typed: *mut PyByteArrayObject = mem::transmute(ptr);
        (*typed).ob_size = value.len() as Py_ssize_t;
        (*typed).ob_exports = 0;
        (*typed).ob_alloc = (*typed).ob_size;
        (*typed).ob_bytes = value.as_mut_ptr();
        assert_eq!(
            PyByteArray_Size(ptr) as usize,
            value.len(),
            "PyByteArray struct mismatch"
        );
        mem::forget(value);
        RustPyObject::from_owned_ptr(py, ptr)
    }
}
