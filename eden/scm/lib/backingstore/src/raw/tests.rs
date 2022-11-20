/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This mod provides utilities functions needed for running tests.

use anyhow::anyhow;

use crate::raw::CBytes;
use crate::raw::CFallible;
use crate::raw::CFallibleBase;

/// Returns a `CFallible` with success return value 1. This function is intended to be called from
/// C++ tests.
#[no_mangle]
pub extern "C" fn sapling_test_cfallible_ok() -> CFallibleBase {
    CFallible::ok(0xFB).into()
}

#[no_mangle]
pub extern "C" fn sapling_test_cfallible_ok_free(val: *mut u8) {
    let x = unsafe { Box::from_raw(val) };
    drop(x);
}

/// Returns a `CFallible` with error message "context: failure!". This function is intended to be called
/// from C++ tests.
#[no_mangle]
pub extern "C" fn sapling_test_cfallible_err() -> CFallibleBase {
    CFallible::<u8>::err(anyhow!("failure!").context("context")).into()
}

#[no_mangle]
pub extern "C" fn sapling_test_cbytes() -> CBytes {
    CBytes::from_vec("hello world".to_string().into_bytes())
}
