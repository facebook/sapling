/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
//! This functionality is provided by the `ContentStore` (for content), and
//! `MetadataStore` (for history) types and are the main entry-points of this
//! library. Both of these have the exact same behavior, they only differ by
//! what they store.
//!
//! The `ContentStore` (and `Metadatastore`) keeps track of data in 2 locations:
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
//! The `ContentStore::add` (from `HgIdMutableDeltaStore`) allows adding data
//! to this store. Care must be taken to call `ContentStore::flush` (from
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
//! ## `MultiplexDeltaStore`, `MultiplexHgIdHistoryStore`
//!
//! Similarly to the `UnionStore`, this allows composing stores together but
//! for the purposes of duplicating all the writes to all the stores. Mainly
//! used to send data to both a fast caching server (ex: Memcache), and to a
//! shared store when receiving network data. It can also be used for data format
//! migration
//!
//! ## `DataPack`, `HistoryPack`
//!
//! Immutable file storage comprised of an index file that tracks the location
//! of the actual data in its associated pack file. Must be repacked frequently
//! to avoid linear searches in them during read operations.
//!
//! On repack, the pack files are squashed together by writing all their data into a
//! `ContentStore` (or `MetadataStore`), which is then committed to disk before the
//! squashed pack files are then deleted from disk. This ensures that a new Mercurial
//! process spawned while repack is running will still be able to read all the data.
//!
//! ## `IndexedLogHgIdDataStore`, `IndexedLogHgIdHistoryStore`
//!
//! Basic `IndexedLog` backed stores. As opposed to the packfiles described above,
//! these allow update in place (append-only).
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
//! `strip_metadata` to obtain the plain blob. Must implement the `LocalStore`
//! trait.
//!
//! ## `ContentDataStore`
//!
//! Implemented by content-only stores. The hash of the returned blob will
//! match exactly the value of the passed in StoreKey hash. Used by the
//! `LfsStore`.
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
//!

mod contentstore;
mod dataindex;
mod edenapi;
#[cfg(all(fbcode_build, target_os = "linux"))]
mod facebook;
mod fanouttable;
mod historyindex;
mod indexedloghistorystore;
mod indexedlogutil;
mod lfs;
mod memcache;
mod metadatastore;
mod remotestore;
mod repack;
mod sliceext;
mod types;
mod unionstore;
mod util;

pub mod c_api;
pub mod datapack;
pub mod datastore;
pub mod error;
pub mod historypack;
pub mod historystore;
pub mod indexedlogdatastore;
pub mod localstore;
pub mod multiplexstore;
pub mod mutabledatapack;
pub mod mutablehistorypack;
pub mod mutablepack;
pub mod packstore;
pub mod packwriter;
pub mod uniondatastore;
pub mod unionhistorystore;

pub use crate::contentstore::{ContentStore, ContentStoreBuilder};
pub use crate::datapack::{DataEntry, DataPack, DataPackVersion};
pub use crate::datastore::{
    ContentDataStore, ContentMetadata, Delta, HgIdDataStore, HgIdMutableDeltaStore, RemoteDataStore,
};
pub use crate::edenapi::EdenApiHgIdRemoteStore;
pub use crate::historypack::{HistoryEntry, HistoryPack, HistoryPackVersion};
pub use crate::historystore::{HgIdHistoryStore, HgIdMutableHistoryStore, RemoteHistoryStore};
pub use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
pub use crate::indexedloghistorystore::IndexedLogHgIdHistoryStore;
pub use crate::localstore::LocalStore;
pub use crate::memcache::MemcacheStore;
pub use crate::metadatastore::{MetadataStore, MetadataStoreBuilder};
pub use crate::multiplexstore::{MultiplexDeltaStore, MultiplexHgIdHistoryStore};
pub use crate::mutabledatapack::MutableDataPack;
pub use crate::mutablehistorypack::MutableHistoryPack;
pub use crate::packstore::{
    CorruptionPolicy, DataPackStore, HistoryPackStore, MutableDataPackStore,
    MutableHistoryPackStore,
};
pub use crate::remotestore::HgIdRemoteStore;
pub use crate::repack::{repack, RepackKind, RepackLocation, Repackable, ToKeys};
pub use crate::types::{ContentHash, StoreKey};
pub use crate::uniondatastore::UnionHgIdDataStore;
pub use crate::util::Error;

pub use indexedlog::Repair as IndexedlogRepair;
pub use revisionstore_types::*;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
