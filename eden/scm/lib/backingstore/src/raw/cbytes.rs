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
    ptr: *const u8,
    len: size_t,
    vec: *mut Vec<u8>,
}

impl CBytes {
    #[allow(dead_code)]
    pub fn from_vec(vec: Vec<u8>) -> Self {
        let vec = Box::new(vec);
        let ptr = vec.as_ptr();

        Self {
            ptr,
            len: vec.len(),
            vec: Box::into_raw(vec),
        }
    }
}

#[no_mangle]
pub extern "C" fn rust_cbytes_free(vec: *mut CBytes) {
    let ptr = unsafe { Box::from_raw(vec) };
    let vec = unsafe { Box::from_raw(ptr.vec) };
    drop(vec);
    drop(ptr);
}
