/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

mod contentstore;
mod dataindex;
mod edenapi;
#[cfg(fbcode_build)]
mod facebook;
mod fanouttable;
mod historyindex;
mod indexedloghistorystore;
mod indexedlogutil;
mod lfs;
mod memcache;
mod metadatastore;
mod remotestore;
mod sliceext;
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
pub mod repack;
pub mod uniondatastore;
pub mod unionhistorystore;

pub use crate::contentstore::{ContentStore, ContentStoreBuilder};
pub use crate::datapack::{DataEntry, DataPack, DataPackVersion};
pub use crate::datastore::{
    Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore,
};
pub use crate::edenapi::EdenApiHgIdRemoteStore;
pub use crate::historypack::{HistoryEntry, HistoryPack, HistoryPackVersion};
pub use crate::historystore::{HistoryStore, MutableHistoryStore, RemoteHistoryStore};
pub use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
pub use crate::indexedloghistorystore::IndexedLogHistoryStore;
pub use crate::localstore::HgIdLocalStore;
pub use crate::memcache::MemcacheStore;
pub use crate::metadatastore::{MetadataStore, MetadataStoreBuilder};
pub use crate::multiplexstore::{MultiplexDeltaStore, MultiplexHistoryStore};
pub use crate::mutabledatapack::MutableDataPack;
pub use crate::mutablehistorypack::MutableHistoryPack;
pub use crate::packstore::{
    CorruptionPolicy, DataPackStore, HistoryPackStore, MutableDataPackStore,
    MutableHistoryPackStore,
};
pub use crate::remotestore::HgIdRemoteStore;
pub use crate::repack::ToKeys;
pub use crate::uniondatastore::UnionHgIdDataStore;
pub use crate::util::Error;

pub use indexedlog::Repair as IndexedlogRepair;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
