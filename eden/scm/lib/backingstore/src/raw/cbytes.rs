/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides a struct to pass a Rust `vec` to C++. However, the C++ code must hold a reference to
//! the underlying Rust `vec` since `Vec::as_ptr` requires the vector remain valid and alive over
//! the lifetime of the pointer it returns.

use libc::size_t;

#[repr(C)]
pub struct CBytes {
    ptr: *mut u8,
    len: size_t,
    vec: *mut Vec<u8>,
}

impl CBytes {
    pub fn from_vec(vec: Vec<u8>) -> Self {
        let mut vec = Box::new(vec);
        let ptr = vec.as_mut_ptr();

        Self {
            ptr,
            len: vec.len(),
            vec: Box::into_raw(vec),
        }
    }
}

impl From<Vec<u8>> for CBytes {
    fn from(vec: Vec<u8>) -> Self {
        CBytes::from_vec(vec)
    }
}

impl Drop for CBytes {
    fn drop(&mut self) {
        let vec = unsafe { Box::from_raw(self.vec) };
        drop(vec);
    }
}

#[no_mangle]
pub extern "C" fn rust_cbytes_free(vec: *mut CBytes) {
    let ptr = unsafe { Box::from_raw(vec) };
    drop(ptr);
}
