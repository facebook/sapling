/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This library provides access to Mercurial's file and tree data. It manages
//! the lifetime of these and takes care of fetching them when not present
//! locally.
//!
//! # High-level overview
//!
//! After cloning a repository, no file and tree data are present locally,
//! these are either created during a `hg commit` operation, or on-demand fetched
//! when accessed.
//!
//! This functionality is provided by the `ScmStore` (for content), and
//! `MetadataStore` (for history) types and are the main entry-points of this
//! library. Both of these have the exact same behavior, they only differ by
//! what they store.
//!
//! The `ScmStore` (and `Metadatastore`) keeps track of data in 2 locations:
//!   - A shared store location,
//!   - A local store location.
//!
//! The shared store is where network fetched data is written into, and to
//! prevent excessive disk usage, is automatically size constrained with the
//! Mercurial config `remotefilelog.cachelimit` (for files) and
//! `remotefilelog.manifestlimit` (for trees). The assumption is that data
//! available on the server will always be available and can be fetched at any
//! time. Writing to this store is done automatically and no APIs are exposed
//! to write to it.
//!
//! The local store is where `hg commit` data goes into. As opposed to the
//! shared store, it is not automatically reclaimed and will grow unbounded.
//! The `ScmStore::add` (from `HgIdMutableDeltaStore`) allows adding data
//! to this store. Care must be taken to call `ScmStore::flush` (from
//! `HgIdMutableDeltaStore`) for the written data to be persisted on disk.
//!
//! # Types
//!
//! ## `Key`
//!
//! A `Key` is comprised of both a filenode hash, and a path. Old style of
//! addressing content.
//!
//! ## `StoreKey`
//!
//! A `StoreKey` allows mixing both `Key` based addressing, and content-only
//! hashed addressing. Used predominantly in the `LfsStore`.
//!
//! ## `UnionStore`
//!
//! Compose multiple stores into one and re-implement the main traits by
//! iterating over these stores.
//!
//! ## `IndexedLogHgIdDataStore`, `IndexedLogHgIdHistoryStore`
//!
//! Basic `IndexedLog` backed stores. As opposed to the packfiles, these allow
//! update in place (append-only).
//!
//! ## `LfsStore`
//!
//! Alternative store for large blobs. Data stored in it is bipartite: one pointer
//! that describe a blob, it's size and other metadata, and the pure blob itself.
//! The blob is addressed by its sha256 only, while the pointer can be retrieved
//! via either the blob hash, or a plain `Key`.
//!
//! # Traits
//!
//! ## `LocalStore`
//!
//! Badly named trait, initially intended to be implemented only by on-disk
//! stores, it provides a `get_missing` API that test whether some data is
//! present in the store. Addressed via a `StoreKey`.
//!
//! ## `HgIdDataStore`, `HgIdHistoryStore`
//!
//! Main interface to read data out of a store. For copied file data, the returned
//! data will contain a copy-from header which may need to be stripped with
//! `strip_file_metadata` to obtain the plain blob. Must implement the `LocalStore`
//! trait. Metadata can be also separated with split_hg_file_metadata that returns raw metadata blob.
//!
//! ## `HgIdMutableDeltaStore`, `HgIdMutableHistoryStore`
//!
//! Main interface to write to a store. Unflushed data may be lost. Must implement
//! the `LocalStore` and `HgIdDataStore`/`HgIdHistoryStore` traits.
//!
//! ## `HgIdRemoteStore`, `RemoteDataStore`, `RemoteHistoryStore`
//!
//! The `HgIdRemoteStore` is implemented by raw remote stores, it is intended
//! to produce individual `RemoteDataStore` or `RemoteHistoryStore`. These stores
//! will automatically write fetched data to the passed mutable stores. This is
//! implemented by both the ssh and the edenapi remote store.
//!
//! The produced stores must implement the `HgIdDataStore` trait.

mod indexedloghistorystore;
mod indexedlogutil;
mod lfs;
mod metadatastore;
mod missing;
mod remotestore;
mod repair;
mod sliceext;
mod types;
mod unionstore;

pub mod datastore;
pub mod edenapi;
pub mod error;
pub mod historystore;
pub mod indexedlogauxstore;
pub mod indexedlogdatastore;
pub mod indexedlogtreeauxstore;
pub mod localstore;
pub mod scmstore;
pub mod trait_impls;
pub mod uniondatastore;
pub mod unionhistorystore;
pub mod util;

use ::types::Key;
pub use revisionstore_types::*;

pub use crate::datastore::ContentMetadata;
pub use crate::datastore::Delta;
pub use crate::datastore::HgIdDataStore;
pub use crate::datastore::HgIdMutableDeltaStore;
pub use crate::datastore::RemoteDataStore;
pub use crate::datastore::StoreResult;
pub use crate::edenapi::SaplingRemoteApiFileStore;
pub use crate::edenapi::SaplingRemoteApiRemoteStore;
pub use crate::edenapi::SaplingRemoteApiTreeStore;
pub use crate::historystore::HgIdHistoryStore;
pub use crate::historystore::HgIdMutableHistoryStore;
pub use crate::historystore::HistoryStore;
pub use crate::historystore::RemoteHistoryStore;
pub use crate::indexedlogauxstore::AuxStore;
pub use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
pub use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
pub use crate::indexedloghistorystore::IndexedLogHgIdHistoryStore;
pub use crate::indexedlogutil::StoreType;
pub use crate::lfs::LfsRemote;
pub use crate::localstore::LocalStore;
pub use crate::metadatastore::MetadataStore;
pub use crate::metadatastore::MetadataStoreBuilder;
pub use crate::remotestore::HgIdRemoteStore;
pub use crate::repair::repair;
pub use crate::types::ContentHash;
pub use crate::types::StoreKey;
pub use crate::uniondatastore::UnionHgIdDataStore;

pub trait ToKeys {
    fn to_keys(&self) -> Vec<anyhow::Result<Key>>;
}

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;

#[cfg(test)]
pub(crate) use env_lock::env_lock;

#[cfg(test)]
mod env_lock {
    use parking_lot::Mutex;

    pub static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_reset() {
        for name in ["https_proxy", "http_proxy", "NO_PROXY"] {
            if std::env::var_os(name).is_some() {
                // TODO: Audit that the environment access only happens in single-threaded code.
                unsafe { std::env::remove_var(name) }
            }
        }
    }

    pub(crate) fn env_lock() -> impl Drop {
        let lock = ENV_LOCK.lock();
        env_reset();
        lock
    }
}
