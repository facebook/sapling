/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::ffi::CStr;
use std::os::raw::c_char;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use cxx::SharedPtr;
use storemodel::FileAuxData as ScmStoreFileAuxData;
use types::fetch_mode::FetchMode;
use types::Key;

use crate::backingstore::BackingStore;

#[cxx::bridge(namespace = sapling)]
pub(crate) mod ffi {
    // see https://cxx.rs/shared.html#extern-enums
    #[namespace = "facebook::eden"]
    #[repr(u8)]
    pub enum FetchCause {
        Unknown,
        // The request originated from FUSE/NFS/PrjFS
        Fs,
        // The request originated from a Thrift endpoint
        Thrift,
        // The request originated from a Thrift prefetch endpoint
        Prefetch,
    }

    #[namespace = "facebook::eden"]
    unsafe extern "C++" {
        include!("eden/fs/store/ObjectFetchContext.h");

        // The above enum
        type FetchCause;
    }

    pub struct SaplingNativeBackingStoreOptions {
        allow_retries: bool,
    }

    #[repr(u8)]
    pub enum FetchMode {
        /// The fetch may hit remote servers.
        AllowRemote,
        /// The fetch is limited to RAM and disk.
        LocalOnly,
        /// The fetch is only hits remote servers.
        RemoteOnly,
        /// The fetch may hit remote servers and should prefetch optional data. For trees,
        /// this means request optional child metadata. This will not trigger a remote child
        /// metadata fetch if the tree is already available locally.
        AllowRemotePrefetch,
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
        cause: FetchCause,
        // TODO: mode: FetchMode
        // TODO: cri: ClientRequestInfo
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
            options: &SaplingNativeBackingStoreOptions,
        ) -> Result<Box<BackingStore>>;

        pub unsafe fn sapling_backingstore_get_name(store: &BackingStore) -> Result<String>;

        pub fn sapling_backingstore_get_manifest(
            store: &mut BackingStore,
            node: &[u8],
        ) -> Result<[u8; 20]>;

        pub fn sapling_backingstore_get_tree(
            store: &BackingStore,
            node: &[u8],
            fetch_mode: FetchMode,
        ) -> Result<SharedPtr<Tree>>;

        pub fn sapling_backingstore_get_tree_batch(
            store: &BackingStore,
            requests: &[Request],
            fetch_mode: FetchMode,
            resolver: SharedPtr<GetTreeBatchResolver>,
        );

        pub fn sapling_backingstore_get_blob(
            store: &BackingStore,
            node: &[u8],
            fetch_mode: FetchMode,
        ) -> Result<Box<Blob>>;

        pub fn sapling_backingstore_get_blob_batch(
            store: &BackingStore,
            requests: &[Request],
            fetch_mode: FetchMode,
            resolver: SharedPtr<GetBlobBatchResolver>,
        );

        pub fn sapling_backingstore_get_file_aux(
            store: &BackingStore,
            node: &[u8],
            fetch_mode: FetchMode,
        ) -> Result<SharedPtr<FileAuxData>>;

        pub fn sapling_backingstore_get_file_aux_batch(
            store: &BackingStore,
            requests: &[Request],
            fetch_mode: FetchMode,
            resolver: SharedPtr<GetFileAuxBatchResolver>,
        );

        pub fn sapling_backingstore_flush(store: &BackingStore);
    }
}

impl From<ffi::FetchMode> for FetchMode {
    fn from(fetch_mode: ffi::FetchMode) -> Self {
        match fetch_mode {
            ffi::FetchMode::AllowRemote => FetchMode::AllowRemote,
            ffi::FetchMode::AllowRemotePrefetch => FetchMode::AllowRemotePrefetch,
            ffi::FetchMode::RemoteOnly => FetchMode::RemoteOnly,
            ffi::FetchMode::LocalOnly => FetchMode::LocalOnly,
            _ => panic!("unknown fetch mode"),
        }
    }
}

pub unsafe fn sapling_backingstore_new(
    repository: &[c_char],
    options: &ffi::SaplingNativeBackingStoreOptions,
) -> Result<Box<BackingStore>> {
    super::init::backingstore_global_init();

    let repo = CStr::from_ptr(repository.as_ptr()).to_str()?;
    let store =
        BackingStore::new(repo, options.allow_retries).map_err(|err| anyhow!("{:?}", err))?;
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
    fetch_mode: ffi::FetchMode,
) -> Result<SharedPtr<ffi::Tree>> {
    Ok(SharedPtr::new(
        store
            .get_tree(node, FetchMode::from(fetch_mode))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")))
            .and_then(|entry| entry.try_into())?,
    ))
}

pub fn sapling_backingstore_get_tree_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetTreeBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();

    store.get_tree_batch(keys, FetchMode::from(fetch_mode), |idx, result| {
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
    fetch_mode: ffi::FetchMode,
) -> Result<Box<ffi::Blob>> {
    let bytes = store
        .get_blob(node, FetchMode::from(fetch_mode))
        .and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")))?;
    Ok(Box::new(ffi::Blob { bytes }))
}

pub fn sapling_backingstore_get_blob_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetBlobBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    store.get_blob_batch(keys, FetchMode::from(fetch_mode), |idx, result| {
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
    fetch_mode: ffi::FetchMode,
) -> Result<SharedPtr<ffi::FileAuxData>> {
    Ok(SharedPtr::new(
        store
            .get_file_aux(node, FetchMode::from(fetch_mode))
            .and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")))?
            .into(),
    ))
}

pub fn sapling_backingstore_get_file_aux_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetFileAuxBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();

    store.get_file_aux_batch(keys, FetchMode::from(fetch_mode), |idx, result| {
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
