/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use minibytes::Bytes as MiniBytes;
#[cfg(feature = "python3")]
use python3_sys as ffi;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "bytes"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<Bytes>(py)?;
    Ok(m)
}

py_class!(pub class Bytes |py| {
    data inner: MiniBytes;

    /// Convert to `memoryview` (Python 3).
    def asref(&self) -> PyResult<PyObject> {
        let slice: &[u8] = self.inner(py).as_ref();

        #[cfg(feature = "python3")]
        unsafe {
            let raw_obj = ffi::PyMemoryView_FromMemory(
                slice.as_ptr() as *const _ as *mut _,
                slice.len() as _,
                ffi::PyBUF_READ,
            );
            return Ok(PyObject::from_owned_ptr(py, raw_obj))
        }
    }
});

impl Bytes {
    /// Convert `minibytes::Bytes` to a Python `Bytes` that implements the
    /// `Py_buffer` interface.
    pub fn from_bytes(py: Python, bytes: MiniBytes) -> PyResult<Self> {
        Self::create_instance(py, bytes)
    }
}
