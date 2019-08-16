// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

mod ancestors;
mod dataindex;
mod fanouttable;
mod historyindex;
mod sliceext;
mod unionstore;
mod vfs;

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

pub use crate::datapack::{DataEntry, DataPack, DataPackVersion};
pub use crate::datastore::{DataStore, Delta, Metadata, MutableDeltaStore};
pub use crate::historypack::{HistoryEntry, HistoryPack, HistoryPackVersion};
pub use crate::historystore::{Ancestors, HistoryStore, MutableHistoryStore};
pub use crate::indexedlogdatastore::IndexedLogDataStore;
pub use crate::localstore::LocalStore;
pub use crate::multiplexstore::{MultiplexDeltaStore, MultiplexHistoryStore};
pub use crate::mutabledatapack::MutableDataPack;
pub use crate::mutablehistorypack::MutableHistoryPack;
pub use crate::packstore::DataPackStore;
pub use crate::repack::IterableStore;
pub use crate::uniondatastore::UnionDataStore;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
