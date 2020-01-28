/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "python3")]
use std::os::raw::c_char;
use std::{mem, slice};

use cpython::{PyObject as RustPyObject, Python as RustPythonGILGuard};
#[cfg(feature = "python2")]
use python27_sys::{
    PyBytesObject, PyBytes_Type, PyObject, PyTypeObject, PyVarObject, Py_ssize_t, _PyObject_NewVar,
};
#[cfg(feature = "python3")]
use python3_sys::{
    PyBytes_Type, PyObject, PyTypeObject, PyVarObject, Py_hash_t, Py_ssize_t, _PyObject_NewVar,
};

// From Python bytesobject.h. Must match the C definition.
#[cfg(feature = "python3")]
#[repr(C)]
struct PyBytesObject {
    pub ob_base: PyVarObject,
    pub ob_shash: Py_hash_t,
    pub ob_sval: [c_char; 1],
}

/// Create a `PyBytes` object that have `size` bytes. Return the object and
/// its internal buffer to be written. This is useful to bypass the memcpy
/// cost creating a large `PyBytesObject`.
pub fn allocate_pybytes(py: RustPythonGILGuard<'_>, size: usize) -> (RustPyObject, &mut [u8]) {
    unsafe {
        let ptr: *mut PyVarObject = _PyObject_NewVar(
            &mut PyBytes_Type as *mut PyTypeObject,
            (size + mem::size_of::<PyBytesObject>()) as Py_ssize_t,
        );
        let mut ptr: *mut PyObject = mem::transmute(ptr);
        let typed: *mut PyBytesObject = mem::transmute(ptr);
        (*typed).ob_shash = -1; // hash not calculated
        #[cfg(feature = "python2")]
        {
            (*typed).ob_sstate = 0; // SSTATE_NOT_INTERNED
            (*typed).ob_size = size as Py_ssize_t;
        }
        #[cfg(feature = "python3")]
        {
            (*typed).ob_base.ob_size = size as Py_ssize_t;
        }
        // Set the first byte to '\0'. If the caller forgot to populate the
        // slice, PyBytes_AsString would still return an empty C string.
        (*typed).ob_sval[0] = 0;
        // Set the byte after the slice to '\0' If the caller does populate the
        // slice, PyBytes_AsString would return a C string with tailing '\0'.
        *((*typed).ob_sval.as_mut_ptr().offset(size as isize)) = 0;
        let pptr: *mut *mut PyObject = &mut ptr;
        ptr = *pptr;
        assert!(!ptr.is_null(), "out of memory");
        let sval: *mut u8 = mem::transmute((*typed).ob_sval.as_mut_ptr());
        let slice = slice::from_raw_parts_mut(sval, size);
        (RustPyObject::from_owned_ptr(py, ptr), slice)
    }
}
