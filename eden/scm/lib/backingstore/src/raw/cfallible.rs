/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides a Result-like struct that can be consumed by C/C++ code.
//!
//! The size of this struct is certain since it only holds pointers.
//!
//! # Memory Management
//!
//! Consumer of this struct needs to ensure the returned error string freed with
//! `rust_cfallible_free_error`.

use failure::Fallible as Result;
use libc::c_char;
use std::ffi::CString;

/// A `repr(C)` struct that can be consumed by C++ code. User of this struct should check
/// `is_error` field to see if there is an error.
///
/// Note: user of this struct is responsible to manage the memory passed through via this struct.
///
/// Note: MSVC toolchain dislikes the usage of template in extern functions. Because of this, we
/// cannot rely on cbindgen to generate the interface for this struct. All changes to this function
/// requires manual editing of the corresponding C++ struct definition in `cbindgen.toml`.
#[repr(C)]
pub struct CFallible<T> {
    value: *mut T,
    error: *mut c_char,
}

impl<T> CFallible<T> {
    /// Creates a `CFallible` with a valid pointer and no error.
    pub fn ok(value: *mut T) -> Self {
        CFallible {
            value,
            error: std::ptr::null_mut(),
        }
    }

    /// Creates a `CFallible` with an error message but no value.
    ///
    /// This function will remove any '\0' in the error message.
    pub fn err<P: ToString>(err: P) -> Self {
        let mut err = err.to_string().into_bytes();
        // `CString::new` will return error only when there is a '\0' in the string. So we manually
        // remove any \0 in the error string to ensure it is safe to call `.expect`.
        err.retain(|&x| x != 0u8);
        let error = CString::new(err).expect("Error message contains \\0");

        CFallible {
            value: std::ptr::null_mut(),
            error: error.into_raw(),
        }
    }
}

impl<T> From<Result<*mut T>> for CFallible<T> {
    fn from(value: Result<*mut T>) -> Self {
        match value {
            Ok(value) => CFallible::ok(value),
            Err(err) => CFallible::err(err),
        }
    }
}

#[no_mangle]
pub extern "C" fn rust_cfallible_free_error(ptr: *mut c_char) {
    let error = unsafe { CString::from_raw(ptr) };
    drop(error);
}
