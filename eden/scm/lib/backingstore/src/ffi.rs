/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::ffi::CStr;
use std::os::raw::c_char;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use cxx::SharedPtr;
use cxx::UniquePtr;
use iobuf::IOBuf;
use storemodel::FileAuxData as ScmStoreFileAuxData;
use types::fetch_cause::FetchCause;
use types::fetch_mode::FetchMode;
use types::FetchContext;
use types::Key;

use crate::backingstore::BackingStore;

#[cxx::bridge(namespace = sapling)]
pub(crate) mod ffi {
    // see https://cxx.rs/shared.html#extern-enums
    #[namespace = "facebook::eden"]
    #[repr(u8)]
    pub enum FetchCause {
        // Lowest Prioity - Unknown orginination
        Unknown,
        // The request originated from a Thrift prefetch endpoint
        Prefetch,
        // The request originated from a Thrift endpoint
        Thrift,
        // Highest Priority - The request originated from FUSE/NFS/PrjFS
        Fs,
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

    #[derive(Debug)]
    pub struct TreeAuxData {
        digest_size: u64,
        digest_hash: [u8; 32],
    }

    pub struct Request {
        node: *const u8,
        cause: FetchCause,
        // TODO: mode: FetchMode
        // TODO: cri: ClientRequestInfo
    }

    pub struct GlobFilesResponse {
        files: Vec<String>,
    }

    pub struct FileAuxData {
        total_size: u64,
        content_sha1: [u8; 20],
        content_blake3: [u8; 32],
    }

    unsafe extern "C++" {
        include!("eden/scm/lib/backingstore/include/ffi.h");

        #[namespace = "folly"]
        type IOBuf = iobuf::IOBuf;

        type GetTreeBatchResolver;
        type GetTreeAuxBatchResolver;
        type GetBlobBatchResolver;
        type GetFileAuxBatchResolver;

        unsafe fn sapling_backingstore_get_tree_batch_handler(
            resolve_state: SharedPtr<GetTreeBatchResolver>,
            index: usize,
            error: String,
            tree: SharedPtr<Tree>,
        );

        unsafe fn sapling_backingstore_get_tree_aux_batch_handler(
            resolve_state: SharedPtr<GetTreeAuxBatchResolver>,
            index: usize,
            error: String,
            tree: SharedPtr<TreeAuxData>,
        );

        unsafe fn sapling_backingstore_get_blob_batch_handler(
            resolve_state: SharedPtr<GetBlobBatchResolver>,
            index: usize,
            error: String,
            blob: UniquePtr<IOBuf>,
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
            store: &BackingStore,
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

        pub fn sapling_backingstore_get_tree_aux(
            store: &BackingStore,
            node: &[u8],
            fetch_mode: FetchMode,
        ) -> Result<SharedPtr<TreeAuxData>>;

        pub fn sapling_backingstore_get_tree_aux_batch(
            store: &BackingStore,
            requests: &[Request],
            fetch_mode: FetchMode,
            resolver: SharedPtr<GetTreeAuxBatchResolver>,
        );

        pub fn sapling_backingstore_get_blob(
            store: &BackingStore,
            node: &[u8],
            fetch_mode: FetchMode,
        ) -> Result<UniquePtr<IOBuf>>;

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

        pub fn sapling_backingstore_get_glob_files(
            store: &BackingStore,
            commit_id: &[u8],
            suffixes: Vec<String>,
            prefixes: Vec<String>,
        ) -> Result<SharedPtr<GlobFilesResponse>>;

        pub fn sapling_dogfooding_host(store: &BackingStore) -> Result<bool>;
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

impl From<ffi::FetchCause> for FetchCause {
    fn from(fetch_cause: ffi::FetchCause) -> Self {
        match fetch_cause {
            ffi::FetchCause::Unknown => FetchCause::EdenUnknown,
            ffi::FetchCause::Prefetch => FetchCause::EdenPrefetch,
            ffi::FetchCause::Thrift => FetchCause::EdenThrift,
            ffi::FetchCause::Fs => FetchCause::EdenFs,
            _ => FetchCause::Unspecified, // should never happen
        }
    }
}

/// Select the most popular cause from a list of causes.
/// If no cause is more than half of the total, return EdenMixed.
fn select_cause(fetch_causes_iter: impl Iterator<Item = ffi::FetchCause>) -> FetchCause {
    let mut most_popular_cause = None;
    let mut len = 0;
    let mut max_count = 0;
    let mut cause_counts = [0; 4]; // 4 is the number of variants in FetchCause enum
    for cause in fetch_causes_iter {
        let cause_index = match cause {
            ffi::FetchCause::Unknown => 0,
            ffi::FetchCause::Prefetch => 1,
            ffi::FetchCause::Thrift => 2,
            ffi::FetchCause::Fs => 3,
            _ => 0, // should never happen
        };
        len += 1;
        cause_counts[cause_index] += 1;
        if cause_counts[cause_index] > max_count {
            max_count = cause_counts[cause_index];
            most_popular_cause = Some(cause);
        }
    }
    match most_popular_cause {
        Some(cause) => {
            if max_count > len / 2 {
                // If the most popular cause is more than half of the total, return it.
                cause.into()
            } else {
                FetchCause::EdenMixed
            }
        }
        None => FetchCause::EdenUnknown,
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

pub fn sapling_backingstore_get_manifest(store: &BackingStore, node: &[u8]) -> Result<[u8; 20]> {
    store.get_manifest(node)
}

pub fn sapling_backingstore_get_tree(
    store: &BackingStore,
    node: &[u8],
    fetch_mode: ffi::FetchMode,
) -> Result<SharedPtr<ffi::Tree>> {
    Ok(
        // the cause is not propagated for this API
        match store.get_tree(
            FetchContext::new_with_cause(FetchMode::from(fetch_mode), FetchCause::EdenUnknown),
            node,
        )? {
            Some(entry) => SharedPtr::new(entry.try_into()?),
            None => SharedPtr::null(),
        },
    )
}

pub fn sapling_backingstore_get_tree_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetTreeBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    let cause = select_cause(requests.iter().map(|req| req.cause));

    store.get_tree_batch(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), cause),
        keys,
        |idx, result| {
            let result: Result<Box<dyn storemodel::TreeEntry>> =
                result.and_then(|opt| opt.ok_or_else(|| Error::msg("no tree found")));
            let resolver = resolver.clone();
            let (error, tree) = match result.and_then(|list| list.try_into()) {
                Ok(tree) => (String::default(), SharedPtr::new(tree)),
                Err(error) => (format!("{:?}", error), SharedPtr::null()),
            };
            unsafe { ffi::sapling_backingstore_get_tree_batch_handler(resolver, idx, error, tree) };
        },
    );
}

pub fn sapling_backingstore_get_tree_aux(
    store: &BackingStore,
    node: &[u8],
    fetch_mode: ffi::FetchMode,
) -> Result<SharedPtr<ffi::TreeAuxData>> {
    // the cause is not propagated for this API
    match store.get_tree_aux(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), FetchCause::EdenUnknown),
        node,
    )? {
        Some(aux) => Ok(SharedPtr::new(aux.into())),
        None => Ok(SharedPtr::null()),
    }
}

pub fn sapling_backingstore_get_tree_aux_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetTreeAuxBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    let cause = select_cause(requests.iter().map(|req| req.cause));

    store.get_tree_aux_batch(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), cause),
        keys,
        |idx, result| {
            let result = result.and_then(|opt| opt.ok_or_else(|| Error::msg("no aux data found")));
            let resolver = resolver.clone();
            let (error, aux) = match result {
                Ok(aux) => (String::default(), SharedPtr::new(aux.into())),
                Err(error) => (format!("{:?}", error), SharedPtr::null()),
            };
            unsafe {
                ffi::sapling_backingstore_get_tree_aux_batch_handler(resolver, idx, error, aux)
            };
        },
    );
}

pub fn sapling_backingstore_get_blob(
    store: &BackingStore,
    node: &[u8],
    fetch_mode: ffi::FetchMode,
) -> Result<UniquePtr<IOBuf>> {
    // the cause is not propagated for this API
    match store.get_blob(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), FetchCause::EdenUnknown),
        node,
    )? {
        Some(blob) => Ok(blob.into_iobuf().into()),
        None => Ok(UniquePtr::null()),
    }
}

pub fn sapling_backingstore_get_blob_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetBlobBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    let cause = select_cause(requests.iter().map(|req| req.cause));

    store.get_blob_batch(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), cause),
        keys,
        |idx, result| {
            let result = result.and_then(|opt| opt.ok_or_else(|| Error::msg("no blob found")));
            let resolver = resolver.clone();
            let (error, blob) = match result {
                Ok(blob) => (String::default(), blob.into_iobuf().into()),
                Err(error) => (format!("{:?}", error), UniquePtr::null()),
            };
            unsafe { ffi::sapling_backingstore_get_blob_batch_handler(resolver, idx, error, blob) };
        },
    );
}

pub fn sapling_backingstore_get_file_aux(
    store: &BackingStore,
    node: &[u8],
    fetch_mode: ffi::FetchMode,
) -> Result<SharedPtr<ffi::FileAuxData>> {
    // the cause is not propagated for this API
    match store.get_file_aux(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), FetchCause::EdenUnknown),
        node,
    )? {
        Some(aux) => Ok(SharedPtr::new(aux.into())),
        None => Ok(SharedPtr::null()),
    }
}

pub fn sapling_backingstore_get_file_aux_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    resolver: SharedPtr<ffi::GetFileAuxBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    let cause = select_cause(requests.iter().map(|req| req.cause));

    store.get_file_aux_batch(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), cause),
        keys,
        |idx, result| {
            let result: Result<ScmStoreFileAuxData> =
                result.and_then(|opt| opt.ok_or_else(|| Error::msg("no file aux data found")));
            let resolver = resolver.clone();
            let (error, aux) = match result {
                Ok(aux) => (String::default(), SharedPtr::new(aux.into())),
                Err(error) => (format!("{:?}", error), SharedPtr::null()),
            };
            unsafe {
                ffi::sapling_backingstore_get_file_aux_batch_handler(resolver, idx, error, aux)
            };
        },
    );
}

pub fn sapling_dogfooding_host(store: &BackingStore) -> Result<bool> {
    store.dogfooding_host()
}

pub fn sapling_backingstore_flush(store: &BackingStore) {
    store.flush();
    store.refresh();
}

pub fn sapling_backingstore_get_glob_files(
    store: &BackingStore,
    commit_id: &[u8],
    suffixes: Vec<String>,
    prefixes: Vec<String>,
) -> Result<SharedPtr<ffi::GlobFilesResponse>> {
    let prefix_opt = match prefixes.len() {
        0 => None,
        _ => Some(prefixes),
    };
    let files = store
        .get_glob_files(commit_id, suffixes, prefix_opt)
        .and_then(|opt| opt.ok_or_else(|| Error::msg("failed to retrieve glob file")))?;
    Ok(SharedPtr::new(ffi::GlobFilesResponse { files }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_cause() {
        let causes = [
            ffi::FetchCause::Unknown,
            ffi::FetchCause::Prefetch,
            ffi::FetchCause::Thrift,
            ffi::FetchCause::Fs,
        ];
        for cause in causes.iter().cloned() {
            let selected = select_cause(std::iter::repeat(cause).take(3));
            // Repeating causes should always return the same cause
            assert_eq!(selected, cause.into());
        }
        let selected = select_cause(
            std::iter::repeat(ffi::FetchCause::Thrift)
                .take(3)
                .chain(std::iter::repeat(ffi::FetchCause::Prefetch).take(2)),
        );

        // Thrift is more popular than Prefetch (> 50%)
        assert_eq!(selected, FetchCause::EdenThrift);

        // There is no single most popular cause
        assert_eq!(select_cause(causes.into_iter()), FetchCause::EdenMixed);

        // Empty causes
        assert_eq!(select_cause(std::iter::empty()), FetchCause::EdenUnknown);
    }
}
