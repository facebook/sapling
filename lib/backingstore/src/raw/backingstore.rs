// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides the c-bindings for `crate::backingstore`.

use failure::{ensure, Fallible};
use libc::{c_char, size_t};
use std::{slice, str};

use crate::backingstore::BackingStore;
use crate::raw::CFallible;

fn stringpiece_to_slice<'a>(ptr: *const c_char, length: size_t) -> Fallible<&'a [u8]> {
    ensure!(!ptr.is_null(), "string ptr is null");
    Ok(unsafe { slice::from_raw_parts(ptr as *const u8, length) })
}

fn backingstore_new(
    repository: *const c_char,
    repository_len: size_t,
) -> Fallible<*mut BackingStore> {
    let repository = stringpiece_to_slice(repository, repository_len)?;
    let repo = str::from_utf8(repository)?;
    let store = Box::new(BackingStore::new(repo)?);

    Ok(Box::into_raw(store))
}

#[no_mangle]
pub extern "C" fn rust_backingstore_new(
    repository: *const c_char,
    repository_len: size_t,
) -> CFallible<BackingStore> {
    backingstore_new(repository, repository_len).into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_free(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { Box::from_raw(store) };
    drop(store);
}
