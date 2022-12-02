/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::str;

use anyhow::Error;
use anyhow::Result;
use libc::c_void;
use manifest::List;
use revisionstore::scmstore::FetchMode;
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

fn fetch_mode_from_local(local: bool) -> FetchMode {
    if local {
        FetchMode::LocalOnly
    } else {
        FetchMode::AllowRemote
    }
}

#[repr(C)]
pub struct BackingStoreOptions {
    allow_retries: bool,
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_new(
    repository: Slice<u8>,
    options: &BackingStoreOptions,
) -> CFallibleBase {
    CFallible::make_with(|| {
        super::init::backingstore_global_init();

        let repo = str::from_utf8(repository.slice())?;
        BackingStore::new(repo, options.allow_retries)
    })
    .into()
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_free(store: *mut BackingStore) {
    assert!(!store.is_null());
    let store = unsafe { Box::from_raw(store) };
    drop(store);
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_get_tree(
    store: &mut BackingStore,
    node: Slice<u8>,
    local: bool,
) -> CFallibleBase {
    CFallible::<Tree>::make_with(|| {
        store
            .get_tree(node.slice(), fetch_mode_from_local(local))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")))
            .and_then(|list| list.try_into())
    })
    .into()
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_get_tree_batch(
    store: &mut BackingStore,
    requests: Slice<Request>,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallibleBase),
) {
    let keys: Vec<Key> = requests.slice().iter().map(|req| req.key()).collect();

    store.get_tree_batch(keys, fetch_mode_from_local(local), |idx, result| {
        let result: Result<List> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")));
        let result: Result<Tree> = result.and_then(|list| list.try_into());
        let result: CFallible<Tree> = result.into();
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_get_blob(
    store: &mut BackingStore,
    node: Slice<u8>,
    local: bool,
) -> CFallibleBase {
    CFallible::make_with(|| {
        store
            .get_blob(node.slice(), fetch_mode_from_local(local))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
            .map(CBytes::from_vec)
    })
    .into()
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_get_blob_batch(
    store: &mut BackingStore,
    requests: Slice<Request>,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallibleBase),
) {
    let keys: Vec<Key> = requests.slice().iter().map(|req| req.key()).collect();
    store.get_blob_batch(keys, fetch_mode_from_local(local), |idx, result| {
        let result: CFallible<CBytes> = result
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))
            .map(CBytes::from_vec)
            .into();
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_get_file_aux(
    store: &mut BackingStore,
    node: Slice<u8>,
    local: bool,
) -> CFallibleBase {
    CFallible::<FileAuxData>::make_with(|| {
        store
            .get_file_aux(node.slice(), fetch_mode_from_local(local))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")))
            .map(|aux| aux.into())
    })
    .into()
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_get_file_aux_batch(
    store: &mut BackingStore,
    requests: Slice<Request>,
    local: bool,
    data: *mut c_void,
    resolve: unsafe extern "C" fn(*mut c_void, usize, CFallibleBase),
) {
    let keys: Vec<Key> = requests.slice().iter().map(|req| req.key()).collect();

    store.get_file_aux_batch(keys, fetch_mode_from_local(local), |idx, result| {
        let result: Result<ScmStoreFileAuxData> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")));
        let result: CFallible<FileAuxData> = result.map(|aux| aux.into()).into();
        unsafe { resolve(data, idx, result.into()) };
    });
}

#[no_mangle]
pub extern "C" fn sapling_backingstore_flush(store: &mut BackingStore) {
    store.flush();
}
