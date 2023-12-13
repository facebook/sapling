/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::ffi::CStr;
use std::os::raw::c_char;

use anyhow::Error;
use anyhow::Result;
use cxx::SharedPtr;
use storemodel::FileAuxData as ScmStoreFileAuxData;
use types::Key;

use crate::backingstore::BackingStore;
use crate::FetchMode;

#[cxx::bridge(namespace = sapling)]
pub(crate) mod ffi {
    pub struct BackingStoreOptions {
        allow_retries: bool,
    }

    #[repr(u8)]
    pub enum TreeEntryType {
        Tree,
        RegularFile,
        ExecutableFile,
        Symlink,
    }

    pub struct TreeEntry {
        hash: [u8; 20],
        name: Vec<u8>,
        ttype: TreeEntryType,
        has_size: bool,
        size: u64,
        has_sha1: bool,
        content_sha1: [u8; 20],
        has_blake3: bool,
        content_blake3: [u8; 32],
    }

    pub struct Tree {
        entries: Vec<TreeEntry>,
    }

    pub struct Request {
        node: *const u8,
    }

    pub struct Blob {
        pub(crate) bytes: Vec<u8>,
    }

    pub struct FileAuxData {
        total_size: u64,
        content_id: [u8; 32],
        content_sha1: [u8; 20],
        content_sha256: [u8; 32],
        has_blake3: bool,
        content_blake3: [u8; 32],
    }

    unsafe extern "C++" {
        include!("eden/scm/lib/backingstore/include/ffi.h");

        type GetTreeBatchResolver;
        type GetBlobBatchResolver;
        type GetFileAuxBatchResolver;

        unsafe fn sapling_backingstore_get_tree_batch_handler(
            resolve_state: SharedPtr<GetTreeBatchResolver>,
            index: usize,
            error: String,
            tree: SharedPtr<Tree>,
        );

        unsafe fn sapling_backingstore_get_blob_batch_handler(
            resolve_state: SharedPtr<GetBlobBatchResolver>,
            index: usize,
            error: String,
            blob: Box<Blob>,
        );

        unsafe fn sapling_backingstore_get_file_aux_batch_handler(
            resolve_state: SharedPtr<GetFileAuxBatchResolver>,
            index: usize,
            error: String,
            blob: SharedPtr<FileAuxData>,
        );
    }

    extern "Rust" {
        type BackingStore;

        pub unsafe fn sapling_backingstore_new(
            repository: &[c_char],
            options: &BackingStoreOptions,
        ) -> Result<Box<BackingStore>>;

        pub unsafe fn sapling_backingstore_get_name(store: &BackingStore) -> Result<String>;

        pub fn sapling_backingstore_get_manifest(
            store: &mut BackingStore,
            node: &[u8],
        ) -> Result<[u8; 20]>;

        pub fn sapling_backingstore_get_tree(
            store: &BackingStore,
            node: &[u8],
            local: bool,
        ) -> Result<SharedPtr<Tree>>;

        pub fn sapling_backingstore_get_tree_batch(
            store: &BackingStore,
            requests: &[Request],
            local: bool,
            resolver: SharedPtr<GetTreeBatchResolver>,
        );

        pub fn sapling_backingstore_get_blob(
            store: &BackingStore,
            node: &[u8],
            local: bool,
        ) -> Result<Box<Blob>>;

        pub fn sapling_backingstore_get_blob_batch(
            store: &BackingStore,
            requests: &[Request],
            local: bool,
            resolver: SharedPtr<GetBlobBatchResolver>,
        );

        pub fn sapling_backingstore_get_file_aux(
            store: &BackingStore,
            node: &[u8],
            local: bool,
        ) -> Result<SharedPtr<FileAuxData>>;

        pub fn sapling_backingstore_get_file_aux_batch(
            store: &BackingStore,
            requests: &[Request],
            local: bool,
            resolver: SharedPtr<GetFileAuxBatchResolver>,
        );

        pub fn sapling_backingstore_flush(store: &BackingStore);
    }
}

fn fetch_mode_from_local(local: bool) -> FetchMode {
    if local {
        FetchMode::LocalOnly
    } else {
        FetchMode::AllowRemote
    }
}

pub unsafe fn sapling_backingstore_new(
    repository: &[c_char],
    options: &ffi::BackingStoreOptions,
) -> Result<Box<BackingStore>> {
    super::init::backingstore_global_init();

    let repo = CStr::from_ptr(repository.as_ptr()).to_str()?;
    let store = BackingStore::new(repo, options.allow_retries)?;
    Ok(Box::new(store))
}

pub unsafe fn sapling_backingstore_get_name(store: &BackingStore) -> Result<String> {
    store.name()
}

pub fn sapling_backingstore_get_manifest(
    store: &mut BackingStore,
    node: &[u8],
) -> Result<[u8; 20]> {
    store.get_manifest(node)
}

pub fn sapling_backingstore_get_tree(
    store: &BackingStore,
    node: &[u8],
    local: bool,
) -> Result<SharedPtr<ffi::Tree>> {
    Ok(SharedPtr::new(
        store
            .get_tree(node, fetch_mode_from_local(local))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")))
            .and_then(|entry| entry.try_into())?,
    ))
}

pub fn sapling_backingstore_get_tree_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    local: bool,
    resolver: SharedPtr<ffi::GetTreeBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();

    store.get_tree_batch(keys, fetch_mode_from_local(local), |idx, result| {
        let result: Result<Box<dyn storemodel::TreeEntry>> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")));
        let resolver = resolver.clone();
        let (error, tree) = match result.and_then(|list| list.try_into()) {
            Ok(tree) => (String::default(), SharedPtr::new(tree)),
            Err(error) => (format!("{:?}", error), SharedPtr::null()),
        };
        unsafe { ffi::sapling_backingstore_get_tree_batch_handler(resolver, idx, error, tree) };
    });
}

pub fn sapling_backingstore_get_blob(
    store: &BackingStore,
    node: &[u8],
    local: bool,
) -> Result<Box<ffi::Blob>> {
    let bytes = store
        .get_blob(node, fetch_mode_from_local(local))
        .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))?;
    Ok(Box::new(ffi::Blob { bytes }))
}

pub fn sapling_backingstore_get_blob_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    local: bool,
    resolver: SharedPtr<ffi::GetBlobBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    store.get_blob_batch(keys, fetch_mode_from_local(local), |idx, result| {
        let result = result.and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")));
        let resolver = resolver.clone();
        let (error, blob) = match result {
            Ok(blob) => (String::default(), Box::new(ffi::Blob { bytes: blob })),
            Err(error) => (
                format!("{:?}", error),
                Box::new(ffi::Blob { bytes: vec![] }),
            ),
        };
        unsafe { ffi::sapling_backingstore_get_blob_batch_handler(resolver, idx, error, blob) };
    });
}

pub fn sapling_backingstore_get_file_aux(
    store: &BackingStore,
    node: &[u8],
    local: bool,
) -> Result<SharedPtr<ffi::FileAuxData>> {
    Ok(SharedPtr::new(
        store
            .get_file_aux(node, fetch_mode_from_local(local))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")))?
            .into(),
    ))
}

pub fn sapling_backingstore_get_file_aux_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    local: bool,
    resolver: SharedPtr<ffi::GetFileAuxBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();

    store.get_file_aux_batch(keys, fetch_mode_from_local(local), |idx, result| {
        let result: Result<ScmStoreFileAuxData> =
            result.and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")));
        let resolver = resolver.clone();
        let (error, aux) = match result {
            Ok(aux) => (String::default(), SharedPtr::new(aux.into())),
            Err(error) => (format!("{:?}", error), SharedPtr::null()),
        };
        unsafe { ffi::sapling_backingstore_get_file_aux_batch_handler(resolver, idx, error, aux) };
    });
}

pub fn sapling_backingstore_flush(store: &BackingStore) {
    store.flush();
    store.refresh();
}
