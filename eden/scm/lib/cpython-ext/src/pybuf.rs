/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! A simple `Py_buffer` wrapper that allows zero-copy reading of Python
//! owned memory.

// The objects in memory have a relationship like:
//
// ```text
//     SimplePyBuf<T>   |        Raw Data     Python object
//     +-----------+    |        +-------+    +-----------+
//     | Py_buffer |    |        |       | <-- owns -- _  |
//     | +-------+ |    |        |       |    +-----------+
//     | | *buf -- points to --> |       |
//     | |  len  | |    |        |       |
//     | +-------+ |    |        +-------+
//     +-----------+    |
//                      |
//      Rust-managed    |   Python-managed
// ```
//
// Notes:
// - Raw data is owned by (embedded in, or pointed by) the Python object.
//   Raw data gets freed when the Python object is destructed.
// - Py_buffer is not a Python object but a Python-defined C struct allowing
//   native code to access "Raw data" directly. When constructing Py_buffer
//   from a Python object, the refcount of that Python object increases.
//   The refcount decreases when Py_buffer gets destructed via PyBuffer_Release.
// - Py_buffer is used to expose the raw pointer and length.
// - Memory alignment is up to the actual implementation of "Python object".
//   For a mmap buffer, the libc mmap function guarantees that.

use std::marker::PhantomData;
use std::mem;
use std::slice;

use cpython::PyObject;
use cpython::Python;
use python3_sys as cpy;

pub struct SimplePyBuf<T>(cpy::Py_buffer, PhantomData<T>);

// Since the buffer is read-only and Python cannot move the raw buffer (because
// we own the Py_buffer struct). It's safe to share and use SimplePyBuf in other
// threads.
unsafe impl<T> Send for SimplePyBuf<T> {}
unsafe impl<T> Sync for SimplePyBuf<T> {}

unsafe fn is_safe_type(obj: &PyObject) -> bool {
    unsafe {
        if cpy::PyByteArray_Check(obj.as_ptr()) != 0 {
            return true;
        }
        if cpy::PyBytes_Check(obj.as_ptr()) != 0 {
            return true;
        }
        {
            if cpy::PyMemoryView_Check(obj.as_ptr()) != 0 {
                return true;
            }
        }
        false
    }
}

impl<T: Copy> SimplePyBuf<T> {
    pub fn new(py: Python<'_>, obj: &PyObject) -> Self {
        // Note about GC on obj:
        //
        // Practically, obj here is some low-level, non-container ones like
        // bytes or memoryview that does not support GC (i.e. do not have
        // Py_TPFLAGS_HAVE_GC set).  refcount is the only way to release them.
        // So no need to pay extra attention on them - SimplePyBuf will get
        // refcount right and that's enough.
        //
        // Otherwise (obj is a container type that does support GC), whoever
        // owns this SimplePyBuf in the Rust world needs to do one of the
        // following:
        //   - implement tp_traverse in its Python class
        //   - call PyObject_GC_UnTrack to let GC ignore obj

        // Note about buffer mutability:
        //
        // The code here wants to access the buffer without taking Python GIL.
        // Therefore `obj` should be a read-only object. That is true for Python
        // bytes or buffer(some_other_immutable_obj). For now, explicitly
        // allow those two types. Beware that `PyBuffer_Check` won't guarantee
        // its inner object is also immutable.
        unsafe {
            if !is_safe_type(obj) {
                let ty = obj.get_type(py);
                panic!("potentially unsafe type for SimplePyBuf: {}", ty.name(py));
            }

            let mut buf = mem::zeroed::<SimplePyBuf<T>>();
            let r = cpy::PyObject_GetBuffer(obj.as_ptr(), &mut buf.0, cpy::PyBUF_SIMPLE);
            if r == -1 {
                panic!("failed to get Py_buffer");
            }
            buf
        }
    }
}

impl<T> AsRef<[T]> for SimplePyBuf<T> {
    #[inline]
    fn as_ref(&self) -> &[T] {
        let len = self.0.len as usize / mem::size_of::<T>();
        unsafe { slice::from_raw_parts(self.0.buf as *const T, len) }
    }
}

impl<T> Drop for SimplePyBuf<T> {
    fn drop(&mut self) {
        let _gil = Python::acquire_gil();
        unsafe { cpy::PyBuffer_Release(&mut self.0) }
    }
}
