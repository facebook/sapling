/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use libc::size_t;

use crate::{EdenApiServerError, TreeChildEntry, TreeEntry};

// Monomorphization for Vec<Result<TreeEntry, EdenApiServerError>>
#[no_mangle]
pub extern "C" fn rust_vec_treeentry_len(
    v: *const Vec<Result<TreeEntry, EdenApiServerError>>,
) -> size_t {
    assert!(!v.is_null());
    let v = unsafe { &*v };
    v.len()
}

#[no_mangle]
pub extern "C" fn rust_vec_treeentry_get(
    v: *const Vec<Result<TreeEntry, EdenApiServerError>>,
    idx: size_t,
) -> *const Result<TreeEntry, EdenApiServerError> {
    assert!(!v.is_null());
    let v = unsafe { &*v };
    &v[idx]
}

// Monomorphization for Vec<Result<TreeChildEntry, EdenApiServerError>>
#[no_mangle]
pub extern "C" fn rust_vec_treechild_len(
    v: *const Vec<Result<TreeChildEntry, EdenApiServerError>>,
) -> size_t {
    assert!(!v.is_null());
    let v = unsafe { &*v };
    v.len()
}

#[no_mangle]
pub extern "C" fn rust_vec_treechild_get(
    v: *const Vec<Result<TreeChildEntry, EdenApiServerError>>,
    idx: size_t,
) -> *const Result<TreeChildEntry, EdenApiServerError> {
    assert!(!v.is_null());
    let v = unsafe { &*v };
    &v[idx]
}
