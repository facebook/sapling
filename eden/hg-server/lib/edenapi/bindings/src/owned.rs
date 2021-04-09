/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! These types contain pointers that were allocated in Rust, and must be freed
//! by Rust code. An `extern "C"` _free function is provided for each type.

use anyhow::Error;

use edenapi::Client;

use crate::{EdenApiServerError, TreeEntry};

#[repr(C)]
pub struct EdenApiClient {
    ptr: *mut Result<Client, Error>,
}

impl From<Result<Client, Error>> for EdenApiClient {
    fn from(v: Result<Client, Error>) -> Self {
        let boxed = Box::new(v);
        Self {
            ptr: Box::into_raw(boxed),
        }
    }
}

impl Drop for EdenApiClient {
    fn drop(&mut self) {
        let boxed = unsafe { Box::from_raw(self.ptr) };
        drop(boxed);
    }
}

#[no_mangle]
pub extern "C" fn rust_edenapiclient_free(v: EdenApiClient) {
    drop(v);
}

#[repr(C)]
pub struct TreeEntryFetch {
    ptr: *mut Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>,
}

impl From<Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>> for TreeEntryFetch {
    fn from(v: Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error>) -> Self {
        let boxed = Box::new(v);
        Self {
            ptr: Box::into_raw(boxed),
        }
    }
}

impl Drop for TreeEntryFetch {
    fn drop(&mut self) {
        let boxed = unsafe { Box::from_raw(self.ptr) };
        drop(boxed);
    }
}

#[no_mangle]
pub extern "C" fn rust_treeentryfetch_free(v: TreeEntryFetch) {
    drop(v);
}

/// A wrapper type for a Box<String>. When into_raw_parts is stabilized, the Box / extra allocation
/// can be removed.
#[repr(C)]
pub struct OwnedString {
    ptr: *mut String,
}

impl From<String> for OwnedString {
    fn from(v: String) -> Self {
        let boxed = Box::new(v);
        Self {
            ptr: Box::into_raw(boxed),
        }
    }
}

impl Drop for OwnedString {
    fn drop(&mut self) {
        let boxed = unsafe { Box::from_raw(self.ptr) };
        drop(boxed);
    }
}

#[no_mangle]
pub extern "C" fn rust_ownedstring_len(s: *const OwnedString) -> usize {
    assert!(!s.is_null());
    let s = unsafe { &*(*s).ptr }.as_str();
    s.len()
}

#[no_mangle]
pub extern "C" fn rust_ownedstring_ptr(s: *const OwnedString) -> *const u8 {
    assert!(!s.is_null());
    let s = unsafe { &*(*s).ptr }.as_str();
    s.as_ptr()
}

#[no_mangle]
pub extern "C" fn rust_ownedstring_free(v: OwnedString) {
    drop(v);
}
