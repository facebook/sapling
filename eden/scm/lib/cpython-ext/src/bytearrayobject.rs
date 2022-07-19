/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::mem;
use std::os::raw::c_int;

use cpython::PyObject as RustPyObject;
use cpython::Python as RustPythonGILGuard;
use ffi::PyByteArray_Size;
use ffi::PyByteArray_Type;
use ffi::PyObject;
use ffi::PyTypeObject;
#[cfg(feature = "python3")]
use ffi::PyVarObject;
use ffi::Py_ssize_t;
use ffi::_PyObject_New;
#[cfg(feature = "python2")]
use python27_sys as ffi;
#[cfg(feature = "python3")]
use python3_sys as ffi;

// From Python bytearrayobject.h. Must match the C definition.
#[cfg(feature = "python2")]
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

#[cfg(feature = "python3")]
#[cfg(Py_38)]
#[repr(C)]
struct PyByteArrayObject {
    pub ob_base: PyVarObject,
    pub ob_alloc: Py_ssize_t,
    pub ob_bytes: *mut u8,
    pub ob_start: *mut u8,
    pub ob_exports: c_int,
}

#[cfg(feature = "python3")]
#[cfg(Py_39)]
#[repr(C)]
struct PyByteArrayObject {
    pub ob_base: PyVarObject,
    pub ob_alloc: Py_ssize_t,
    pub ob_bytes: *mut u8,
    pub ob_start: *mut u8,
    pub ob_exports: Py_ssize_t,
}

/// Consume a `Vec<u8>`. Create a Python `bytearray` object.
/// Bytes stored are not copied.
pub fn vec_to_pyobj(py: RustPythonGILGuard<'_>, mut value: Vec<u8>) -> RustPyObject {
    unsafe {
        let ptr: *mut PyObject = _PyObject_New(&mut PyByteArray_Type as *mut PyTypeObject);
        let typed: *mut PyByteArrayObject = mem::transmute(ptr);
        #[cfg(feature = "python2")]
        {
            (*typed).ob_size = value.len() as Py_ssize_t;
        }
        (*typed).ob_exports = 0;
        (*typed).ob_alloc = value.capacity() as Py_ssize_t;
        (*typed).ob_bytes = value.as_mut_ptr();
        #[cfg(feature = "python3")]
        {
            (*typed).ob_base.ob_size = value.len() as Py_ssize_t;
            (*typed).ob_start = (*typed).ob_bytes;
        }
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
        #[cfg(feature = "python2")]
        {
            (*typed).ob_size = value.len() as Py_ssize_t;
        }
        (*typed).ob_exports = 0;
        (*typed).ob_alloc = value.len() as Py_ssize_t;
        (*typed).ob_bytes = value.as_mut_ptr();
        #[cfg(feature = "python3")]
        {
            (*typed).ob_base.ob_size = value.len() as Py_ssize_t;
            (*typed).ob_start = value.as_mut_ptr();
        }
        assert_eq!(
            PyByteArray_Size(ptr) as usize,
            value.len(),
            "PyByteArray struct mismatch"
        );
        mem::forget(value);
        RustPyObject::from_owned_ptr(py, ptr)
    }
}
