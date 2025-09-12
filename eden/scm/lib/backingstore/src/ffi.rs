/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Provides the c-bindings for `crate::backingstore`.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::pin::Pin;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use cxx::SharedPtr;
use cxx::UniquePtr;
use iobuf::IOBuf;
use storemodel::FileAuxData as ScmStoreFileAuxData;
use storemodel::FileType;
use storemodel::TreeEntry;
use storemodel::TreeItemFlag;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::fetch_cause::FetchCause;
use types::fetch_mode::FetchMode;

use crate::backingstore::BackingStore;

#[cxx::bridge(namespace = sapling)]
pub(crate) mod ffi {
    // see https://cxx.rs/shared.html#extern-enums
    #[namespace = "facebook::eden"]
    #[repr(u8)]
    pub enum FetchCause {
        // Lowest Priority - Unknown orginination
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

    #[repr(u8)]
    pub enum FetchMode {
        /// The fetch may hit remote servers.
        AllowRemote,
        /// The fetch is limited to RAM and disk.
        LocalOnly,
        /// The fetch is only hits remote servers.
        RemoteOnly,
    }

    #[namespace = "facebook::eden"]
    #[repr(u8)]
    pub enum TreeEntryType {
        TREE,
        REGULAR_FILE,
        EXECUTABLE_FILE,
        SYMLINK,
    }

    #[namespace = "facebook::eden"]
    unsafe extern "C++" {
        include!("eden/fs/model/TreeEntry.h");

        type TreeEntryType;
    }

    #[derive(Debug)]
    pub struct TreeAuxData {
        digest_size: u64,
        digest_hash: [u8; 32],
    }

    pub struct Request<'a> {
        node: *const u8,
        cause: FetchCause,

        // Only populated for get_tree_batch
        path: &'a [u8],
        oid: &'a [u8],
        pid: u32,
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

    #[namespace = "facebook::eden"]
    #[repr(u8)]
    pub enum HgObjectIdFormat {
        WithPath,
        HashOnly,
    }

    #[namespace = "facebook::eden"]
    unsafe extern "C++" {
        include!("eden/fs/config/HgObjectIdFormat.h");

        type HgObjectIdFormat;
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
            builder: UniquePtr<TreeBuilder>,
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

    unsafe extern "C++" {
        type TreeBuilder;

        fn new_builder(
            case_sensitive: bool,
            oid_format: HgObjectIdFormat,
            oid: &[u8],
            path: &[u8],
        ) -> UniquePtr<TreeBuilder>;

        fn add_entry(
            self: Pin<&mut TreeBuilder>,
            name: &str,
            hg_node: &[u8; 20],
            ttype: TreeEntryType,
        );

        fn add_entry_with_aux_data(
            self: Pin<&mut TreeBuilder>,
            name: &str,
            hg_node: &[u8; 20],
            ttype: TreeEntryType,
            size: u64,
            sha1: &[u8; 20],
            blake3: &[u8; 32],
        );

        fn mark_missing(self: Pin<&mut TreeBuilder>);

        fn reserve(self: Pin<&mut TreeBuilder>, capacity: usize);

        fn set_aux_data(self: Pin<&mut TreeBuilder>, digest: &[u8; 32], size: u64);

        fn num_files(&self) -> usize;

        fn num_dirs(&self) -> usize;
    }

    extern "Rust" {
        type BackingStore;

        pub unsafe fn sapling_backingstore_new(
            repository: &[c_char],
            mount: &[c_char],
        ) -> Result<Box<BackingStore>>;

        pub unsafe fn sapling_backingstore_get_name(store: &BackingStore) -> Result<String>;

        pub fn sapling_backingstore_get_manifest(
            store: &BackingStore,
            node: &[u8],
        ) -> Result<[u8; 20]>;

        pub fn sapling_backingstore_get_tree(
            store: &BackingStore,
            node: &[u8],
            tree_builder: Pin<&mut TreeBuilder>,
            fetch_mode: FetchMode,
        ) -> Result<()>;

        pub fn sapling_backingstore_get_tree_batch(
            store: &BackingStore,
            requests: &[Request],
            fetch_mode: FetchMode,
            oid_format: HgObjectIdFormat,
            case_sensitive: bool,
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

        pub fn sapling_backingstore_witness_file_read(
            store: &BackingStore,
            path: &str,
            local: bool,
            pid: u32,
        );

        pub fn sapling_backingstore_witness_dir_read(
            store: &BackingStore,
            path: &[u8],
            num_files: usize,
            num_dirs: usize,
            local: bool,
            pid: u32,
        );

        pub fn sapling_dogfooding_host(store: &BackingStore) -> Result<bool>;

        pub fn sapling_backingstore_set_parent_hint(store: &BackingStore, parent_id: &str);

        pub fn sapling_flush_counters();
    }
}

impl From<ffi::FetchMode> for FetchMode {
    fn from(fetch_mode: ffi::FetchMode) -> Self {
        match fetch_mode {
            ffi::FetchMode::AllowRemote => FetchMode::AllowRemote,
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
/// Bool return value is `true` if all fetch causes are the same.
fn select_cause(fetch_causes_iter: impl Iterator<Item = ffi::FetchCause>) -> (FetchCause, bool) {
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
                (cause.into(), max_count == len)
            } else {
                (FetchCause::EdenMixed, false)
            }
        }
        None => (FetchCause::EdenUnknown, false),
    }
}

pub unsafe fn sapling_backingstore_new(
    repository: &[c_char],
    mount: &[c_char],
) -> Result<Box<BackingStore>> {
    unsafe {
        super::init::backingstore_global_init();

        let repo = CStr::from_ptr(repository.as_ptr()).to_str()?;
        let mount = CStr::from_ptr(mount.as_ptr()).to_str()?;
        let store = BackingStore::new(repo, mount).map_err(|err| anyhow!("{:?}", err))?;
        Ok(Box::new(store))
    }
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
    tree_builder: Pin<&mut ffi::TreeBuilder>,
    fetch_mode: ffi::FetchMode,
) -> Result<()> {
    // the cause is not propagated for this API
    match store.get_tree(
        FetchContext::new_with_cause(FetchMode::from(fetch_mode), FetchCause::EdenUnknown),
        node,
    )? {
        Some(entry) => add_tree_to_builder(tree_builder, entry),
        None => {
            tree_builder.mark_missing();
            Ok(())
        }
    }
}

// Convert the `TreeEntry` trait object into an EdenFS Tree by adding each entry to the TreeBuilder
// object.
fn add_tree_to_builder(
    mut builder: Pin<&mut ffi::TreeBuilder>,
    tree: Box<dyn TreeEntry>,
) -> Result<()> {
    // TODO: Make the aux data available in `TreeEntry::iter()` so we don't have to do this HashMap business.
    let aux_map: HashMap<HgId, ScmStoreFileAuxData> =
        tree.file_aux_iter()?.collect::<Result<_>>()?;

    // Pre-allocate the per-entry storage.
    if let Some(hint) = tree.size_hint() {
        builder.as_mut().reserve(hint);
    }

    for entry in tree.iter()? {
        let (name, node, flag) = entry?;

        let ttype = match flag {
            TreeItemFlag::Directory => ffi::TreeEntryType::TREE,
            TreeItemFlag::File(FileType::Regular) => ffi::TreeEntryType::REGULAR_FILE,
            TreeItemFlag::File(FileType::Executable) => ffi::TreeEntryType::EXECUTABLE_FILE,
            TreeItemFlag::File(FileType::Symlink) => ffi::TreeEntryType::SYMLINK,
            TreeItemFlag::File(FileType::GitSubmodule) => {
                // Submodules shouldn't show up, and we don't have a way to handle them, so just skip.
                tracing::warn!(%name, %node, "skipping git submodule tree entry");
                continue;
            }
        };

        if let Some(aux) = aux_map.get(&node) {
            builder.as_mut().add_entry_with_aux_data(
                name.as_str(),
                node.as_byte_array(),
                ttype,
                aux.total_size,
                aux.sha1.as_byte_array(),
                aux.blake3.as_byte_array(),
            );
        } else {
            builder
                .as_mut()
                .add_entry(name.as_str(), node.as_byte_array(), ttype);
        }
    }

    if let Some(aux) = tree.aux_data()? {
        builder.as_mut().set_aux_data(
            aux.augmented_manifest_id.as_byte_array(),
            aux.augmented_manifest_size,
        );
    }

    Ok(())
}

pub fn sapling_backingstore_get_tree_batch(
    store: &BackingStore,
    requests: &[ffi::Request],
    fetch_mode: ffi::FetchMode,
    oid_format: ffi::HgObjectIdFormat,
    case_sensitive: bool,
    resolver: SharedPtr<ffi::GetTreeBatchResolver>,
) {
    let keys: Vec<Key> = requests.iter().map(|req| req.key()).collect();
    let cause = select_cause(requests.iter().map(|req| req.cause)).0;
    let fetch_mode = FetchMode::from(fetch_mode);

    store.get_tree_batch(
        FetchContext::new_with_cause(fetch_mode, cause),
        keys,
        |idx, result| {
            let req = &requests[idx];

            let mut builder = ffi::new_builder(case_sensitive, oid_format, req.oid, req.path);

            let error = match result {
                Ok(Some(sl_tree)) => {
                    if let Err(err) = add_tree_to_builder(builder.pin_mut(), sl_tree) {
                        format!("{err:?}")
                    } else {
                        String::default()
                    }
                }
                Ok(None) => {
                    builder.pin_mut().mark_missing();
                    String::default()
                }
                Err(err) => format!("{err:?}"),
            };

            let resolver = resolver.clone();

            if req.cause != ffi::FetchCause::Prefetch && !req.path.is_empty() && error.is_empty() {
                sapling_backingstore_witness_dir_read(
                    store,
                    req.path,
                    builder.num_files(),
                    builder.num_dirs(),
                    fetch_mode.is_local(),
                    req.pid,
                );
            }

            unsafe {
                ffi::sapling_backingstore_get_tree_batch_handler(resolver, idx, error, builder)
            };
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
    let cause = select_cause(requests.iter().map(|req| req.cause)).0;

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
    let cause = select_cause(requests.iter().map(|req| req.cause)).0;

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
    let cause = select_cause(requests.iter().map(|req| req.cause)).0;

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

pub fn sapling_backingstore_set_parent_hint(store: &BackingStore, parent_id: &str) {
    store.set_parent_hint(parent_id);
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

pub fn sapling_backingstore_witness_file_read(
    store: &BackingStore,
    path: &str,
    local: bool,
    pid: u32,
) {
    // `path` comes from eden's RelativePath type, which validates for utf-8 and illegal chars.
    // This is performance sensitive code, so skip the checks.
    store.witness_file_read(unsafe { RepoPath::from_str_unchecked(path) }, local, pid);
}

pub fn sapling_backingstore_witness_dir_read(
    store: &BackingStore,
    path: &[u8],
    num_files: usize,
    num_dirs: usize,
    local: bool,
    pid: u32,
) {
    store.witness_dir_read(
        // `path` comes from eden's RelativePath type, which validates for utf-8 and illegal chars.
        // This is performance sensitive code, so skip the checks.
        unsafe { RepoPath::from_str_unchecked(str::from_utf8_unchecked(path)) },
        local,
        num_files,
        num_dirs,
        pid,
    );
}

pub fn sapling_flush_counters() {
    metrics::Registry::global().sync();
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
            let selected = select_cause(std::iter::repeat_n(cause, 3)).0;
            // Repeating causes should always return the same cause
            assert_eq!(selected, cause.into());
        }
        let selected = select_cause(
            std::iter::repeat_n(ffi::FetchCause::Thrift, 3)
                .chain(std::iter::repeat_n(ffi::FetchCause::Prefetch, 2)),
        );

        // Thrift is more popular than Prefetch (> 50%)
        assert_eq!(selected, (FetchCause::EdenThrift, false));

        // There is no single most popular cause
        assert_eq!(
            select_cause(causes.into_iter()),
            (FetchCause::EdenMixed, false)
        );

        // Empty causes
        assert_eq!(
            select_cause(std::iter::empty()),
            (FetchCause::EdenUnknown, false)
        );

        // All the same cause - return `true`.
        let selected = select_cause(std::iter::repeat_n(ffi::FetchCause::Prefetch, 5));
        assert_eq!(selected, (FetchCause::EdenPrefetch, true));
    }
}
