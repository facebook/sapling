/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides the c-bindings for `crate::backingstore`.

use anyhow::{ensure, Error, Result};
use libc::{c_char, c_void, size_t};
use std::convert::TryInto;
use std::vec::Vec;
use std::{slice, str};
use types::Key;

use crate::backingstore::BackingStore;
use crate::raw::{CBytes, CFallible, Request, Tree};

fn stringpiece_to_slice<'a, T, U>(ptr: *const T, length: size_t) -> Result<&'a [U]> {
    ensure!(!ptr.is_null(), "string ptr is null");
    Ok(unsafe { slice::from_raw_parts(ptr as *const U, length) })
}

fn backingstore_new(
    repository: *const c_char,
    repository_len: size_t,
    use_edenapi: bool,
) -> Result<*mut BackingStore> {
    super::init::backingstore_global_init();

    let repository = stringpiece_to_slice(repository, repository_len)?;
    let repo = str::from_utf8(repository)?;
    let store = Box::new(BackingStore::new(repo, use_edenapi)?);

    Ok(Box::into_raw(store))
}

#[no_mangle]
pub extern "C" fn rust_backingstore_new(
    repository: *const c_char,
    repository_len: size_t,
    use_edenapi: bool,
) -> CFallible<BackingStore> {
    backingstore_new(repository, repository_len, use_edenapi).into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_free(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { Box::from_raw(store) };
    drop(store);
}

fn backingstore_get_blob(
    store: *mut BackingStore,
    name: *const u8,
    name_len: usize,
    node: *const u8,
    node_len: usize,
    local: bool,
) -> Result<*mut CBytes> {
    assert!(!store.is_null());
    let store = unsafe { &*store };
    let path = stringpiece_to_slice(name, name_len)?;
    let node = stringpiece_to_slice(node, node_len)?;

    store
        .get_blob(path, node, local)
        .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
        .map(CBytes::from_vec)
        .map(|result| Box::into_raw(Box::new(result)))
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_blob(
    store: *mut BackingStore,
    name: *const u8,
    name_len: usize,
    node: *const u8,
    node_len: usize,
    local: bool,
) -> CFallible<CBytes> {
    backingstore_get_blob(store, name, name_len, node, node_len, local).into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_blob_batch(
    store: *mut BackingStore,
    requests: *const Request,
    size: usize,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallible<CBytes>),
) {
    assert!(!store.is_null());
    let store = unsafe { &*store };
    let requests: &[Request] = unsafe { slice::from_raw_parts(requests, size) };
    let keys: Vec<Result<Key>> = requests.iter().map(|req| req.try_into_key()).collect();

    store.get_blob_batch(keys, local, |idx, result| {
        let result = result
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
            .map(CBytes::from_vec)
            .map(|result| Box::into_raw(Box::new(result)));
        unsafe { resolve(data, idx, result.into()) };
    });
}

fn backingstore_get_tree(
    store: *mut BackingStore,
    node: *const u8,
    node_len: usize,
) -> Result<*mut Tree> {
    assert!(!store.is_null());
    let store = unsafe { &*store };
    let node = stringpiece_to_slice(node, node_len)?;

    store
        .get_tree(node)
        .and_then(|list| list.try_into())
        .map(|result| Box::into_raw(Box::new(result)))
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_tree(
    store: *mut BackingStore,
    node: *const u8,
    node_len: usize,
) -> CFallible<Tree> {
    backingstore_get_tree(store, node, node_len).into()
}

#[no_mangle]
pub extern "C" fn rust_tree_free(tree: *mut Tree) {
    assert!(!tree.is_null());
    let tree = unsafe { Box::from_raw(tree) };
    drop(tree);
}

#[no_mangle]
pub extern "C" fn rust_backingstore_refresh(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { &*store };

    store.refresh();
}
