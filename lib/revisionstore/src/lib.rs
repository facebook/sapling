// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

mod ancestors;
mod dataindex;
mod fanouttable;
mod historyindex;
mod sliceext;
mod unionstore;

pub mod c_api;
pub mod datapack;
pub mod datastore;
pub mod error;
pub mod historypack;
pub mod historystore;
pub mod key;
pub mod loosefile;
pub mod mutabledatapack;
pub mod mutablehistorypack;
pub mod mutablepack;
pub mod packwriter;
pub mod repack;
pub mod uniondatastore;
pub mod unionhistorystore;

pub use crate::datapack::{DataEntry, DataPack, DataPackVersion};
pub use crate::datastore::{DataStore, Delta, Metadata};
pub use crate::historypack::{HistoryEntry, HistoryPack, HistoryPackVersion};
pub use crate::historystore::{Ancestors, HistoryStore, NodeInfo};
pub use crate::mutabledatapack::MutableDataPack;
pub use crate::mutablehistorypack::MutableHistoryPack;
pub use crate::mutablepack::MutablePack;
