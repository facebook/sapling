/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This mod provides utilities functions needed for running tests.

use crate::raw::{CBytes, CFallible};

/// Returns a `CFallible` with success return value 1. This function is intended to be called from
/// C++ tests.
#[no_mangle]
pub extern "C" fn rust_test_cfallible_ok() -> CFallible<u8> {
    CFallible::ok(Box::into_raw(Box::new(0xFB)))
}

#[no_mangle]
pub extern "C" fn rust_test_cfallible_ok_free(val: *mut u8) {
    let x = unsafe { Box::from_raw(val) };
    drop(x);
}

/// Returns a `CFallible` with error message "failure!". This function is intended to be called
/// from C++ tests.
#[no_mangle]
pub extern "C" fn rust_test_cfallible_err() -> CFallible<u8> {
    CFallible::err("failure!")
}

#[no_mangle]
pub extern "C" fn rust_test_cbytes() -> CBytes {
    CBytes::from_vec("hello world".to_string().into_bytes())
}
