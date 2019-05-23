// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{PyObject as RustPyObject, Python as RustPythonGILGuard};
use python27_sys::{
    PyBytesObject, PyBytes_Type, PyObject, PyTypeObject, PyVarObject, Py_ssize_t, _PyObject_NewVar,
};
use std::mem;
use std::slice;

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
        (*typed).ob_sstate = 0; // SSTATE_NOT_INTERNED
        (*typed).ob_size = size as Py_ssize_t;
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
