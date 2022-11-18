/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::slice;
use std::str;

use anyhow::Error;
use anyhow::Result;
use libc::c_void;
use manifest::List;
use revisionstore::scmstore::FileAuxData as ScmStoreFileAuxData;
use types::Key;

use crate::backingstore::BackingStore;
use crate::raw::CBytes;
use crate::raw::CFallible;
use crate::raw::CFallibleBase;
use crate::raw::FileAuxData;
use crate::raw::Request;
use crate::raw::Slice;
use crate::raw::Tree;

#[repr(C)]
pub struct BackingStoreOptions {
    aux_data: bool,
    allow_retries: bool,
}

#[no_mangle]
pub extern "C" fn rust_backingstore_new(
    repository: Slice<u8>,
    options: &BackingStoreOptions,
) -> CFallibleBase {
    CFallible::make_with(|| {
        super::init::backingstore_global_init();

        let repo = str::from_utf8(repository.slice())?;
        BackingStore::new(repo, options.aux_data, options.allow_retries)
    })
    .into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_free(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { Box::from_raw(store) };
    drop(store);
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_blob(
    store: &mut BackingStore,
    name: Slice<u8>,
    node: Slice<u8>,
    local: bool,
) -> CFallibleBase {
    CFallible::make_with(|| {
        store
            .get_blob(name.slice(), node.slice(), local)
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
            .map(CBytes::from_vec)
    })
    .into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_blob_batch(
    store: &mut BackingStore,
    requests: *const Request,
    size: usize,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallibleBase),
) {
    let requests: &[Request] = unsafe { slice::from_raw_parts(requests, size) };
    let keys: Vec<Result<Key>> = requests.iter().map(|req| req.try_into_key()).collect();
    store.get_blob_batch(keys, local, |idx, result| {
        let result: CFallible<CBytes> = result
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
            .map(CBytes::from_vec)
            .into();
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_tree(
    store: &mut BackingStore,
    node: Slice<u8>,
    local: bool,
) -> CFallibleBase {
    CFallible::<Tree>::make_with(|| {
        store
            .get_tree(node.slice(), local)
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")))
            .and_then(|list| list.try_into())
    })
    .into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_tree_batch(
    store: &mut BackingStore,
    requests: *const Request,
    size: usize,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallibleBase),
) {
    let requests: &[Request] = unsafe { slice::from_raw_parts(requests, size) };
    let keys: Vec<Result<Key>> = requests.iter().map(|req| req.try_into_key()).collect();

    store.get_tree_batch(keys, local, |idx, result| {
        let result: Result<List> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")));
        let result: Result<Tree> = result.and_then(|list| list.try_into());
        let result: CFallible<Tree> = result.into();
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_file_aux(
    store: &mut BackingStore,
    node: Slice<u8>,
    local: bool,
) -> CFallibleBase {
    CFallible::<FileAuxData>::make_with(|| {
        store
            .get_file_aux(node.slice(), local)
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")))
            .map(|aux| aux.into())
    })
    .into()
}

#[no_mangle]
pub extern "C" fn rust_backingstore_get_file_aux_batch(
    store: &mut BackingStore,
    requests: *const Request,
    size: usize,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallibleBase),
) {
    let requests: &[Request] = unsafe { slice::from_raw_parts(requests, size) };
    let keys: Vec<Result<Key>> = requests.iter().map(|req| req.try_into_key()).collect();

    store.get_file_aux_batch(keys, local, |idx, result| {
        let result: Result<ScmStoreFileAuxData> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")));
        let result: CFallible<FileAuxData> = result.map(|aux| aux.into()).into();
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
pub extern "C" fn rust_backingstore_flush(store: &mut BackingStore) {
    store.flush();
}
