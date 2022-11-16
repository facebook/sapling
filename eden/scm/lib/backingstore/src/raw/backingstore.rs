/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::slice;
use std::str;

use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use libc::c_char;
use libc::c_void;
use libc::size_t;
use manifest::List;
use revisionstore::scmstore::FileAuxData as ScmStoreFileAuxData;
use types::Key;

use crate::backingstore::BackingStore;
use crate::raw::CBytes;
use crate::raw::CFallible;
use crate::raw::FileAuxData;
use crate::raw::Request;
use crate::raw::Tree;

fn stringpiece_to_slice<'a, T, U>(ptr: *const T, length: size_t) -> Result<&'a [U]> {
    ensure!(!ptr.is_null(), "string ptr is null");
    Ok(unsafe { slice::from_raw_parts(ptr as *const U, length) })
}

#[no_mangle]
pub extern "C" fn rust_backingstore_new(
    repository: *const c_char,
    repository_len: size_t,
    aux_data: bool,
    allow_retries: bool,
) -> CFallible<BackingStore> {
    CFallible::make_with(|| {
        super::init::backingstore_global_init();

        let repository = stringpiece_to_slice(repository, repository_len)?;
        let repo = str::from_utf8(repository)?;
        BackingStore::new(repo, aux_data, allow_retries)
    })
}

#[no_mangle]
pub extern "C" fn rust_backingstore_free(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { Box::from_raw(store) };
    drop(store);
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
    CFallible::make_with(|| {
        assert!(!store.is_null());
        let store = unsafe { &*store };
        let path = stringpiece_to_slice(name, name_len)?;
        let node = stringpiece_to_slice(node, node_len)?;

        store
            .get_blob(path, node, local)
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
            .map(CBytes::from_vec)
    })
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

#[no_mangle]
pub extern "C" fn rust_backingstore_get_tree(
    store: *mut BackingStore,
    node: *const u8,
    node_len: usize,
    local: bool,
) -> CFallible<Tree> {
    CFallible::make_with(|| {
        assert!(!store.is_null());
        let store = unsafe { &*store };
        let node = stringpiece_to_slice(node, node_len)?;

        store
            .get_tree(node, local)
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")))
            .and_then(|list| list.try_into())
    })
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_tree_batch(
    store: *mut BackingStore,
    requests: *const Request,
    size: usize,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallible<Tree>),
) {
    assert!(!store.is_null());
    let store = unsafe { &*store };
    let requests: &[Request] = unsafe { slice::from_raw_parts(requests, size) };
    let keys: Vec<Result<Key>> = requests.iter().map(|req| req.try_into_key()).collect();

    store.get_tree_batch(keys, local, |idx, result| {
        let result: Result<List> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")));
        let result: Result<Tree> = result.and_then(|list| list.try_into());
        let result: Result<*mut Tree> = result.map(|result| Box::into_raw(Box::new(result)));
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_file_aux(
    store: *mut BackingStore,
    node: *const u8,
    node_len: usize,
    local: bool,
) -> CFallible<FileAuxData> {
    CFallible::make_with(|| {
        assert!(!store.is_null());
        let store = unsafe { &*store };
        let node = stringpiece_to_slice(node, node_len)?;

        store
            .get_file_aux(node, local)
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")))
            .map(|aux| aux.into())
    })
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_file_aux_batch(
    store: *mut BackingStore,
    requests: *const Request,
    size: usize,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallible<FileAuxData>),
) {
    assert!(!store.is_null());
    let store = unsafe { &*store };
    let requests: &[Request] = unsafe { slice::from_raw_parts(requests, size) };
    let keys: Vec<Result<Key>> = requests.iter().map(|req| req.try_into_key()).collect();

    store.get_file_aux_batch(keys, local, |idx, result| {
        let result: Result<ScmStoreFileAuxData> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")));
        let result: Result<FileAuxData> = result.map(|aux| aux.into());
        let result: Result<*mut FileAuxData> = result.map(|result| Box::into_raw(Box::new(result)));
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn rust_tree_free(tree: *mut Tree) {
    assert!(!tree.is_null());
    let tree = unsafe { Box::from_raw(tree) };
    drop(tree);
}

#[no_mangle]
pub extern "C" fn rust_file_aux_free(aux: *mut FileAuxData) {
    assert!(!aux.is_null());
    let aux = unsafe { Box::from_raw(aux) };
    drop(aux);
}

#[no_mangle]
pub extern "C" fn rust_backingstore_flush(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { &*store };

    store.flush();
}
