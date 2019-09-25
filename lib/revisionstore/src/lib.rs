// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

mod ancestors;
mod contentstore;
mod dataindex;
mod edenapi;
mod fanouttable;
mod historyindex;
mod indexedloghistorystore;
mod indexedlogutil;
mod sliceext;
mod unionstore;

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

pub use crate::contentstore::ContentStore;
pub use crate::datapack::{DataEntry, DataPack, DataPackVersion};
pub use crate::datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore};
pub use crate::historypack::{HistoryEntry, HistoryPack, HistoryPackVersion};
pub use crate::historystore::{Ancestors, HistoryStore, MutableHistoryStore};
pub use crate::indexedlogdatastore::IndexedLogDataStore;
pub use crate::indexedloghistorystore::IndexedLogHistoryStore;
pub use crate::localstore::LocalStore;
pub use crate::multiplexstore::{MultiplexDeltaStore, MultiplexHistoryStore};
pub use crate::mutabledatapack::MutableDataPack;
pub use crate::mutablehistorypack::MutableHistoryPack;
pub use crate::packstore::{
    CorruptionPolicy, DataPackStore, HistoryPackStore, MutableDataPackStore,
    MutableHistoryPackStore,
};
pub use crate::repack::ToKeys;
pub use crate::uniondatastore::UnionDataStore;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
