// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

extern crate byteorder;
extern crate crypto;
#[macro_use]
extern crate failure;
extern crate lz4_pyframe;
extern crate memmap;
extern crate tempfile;
extern crate types;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;
#[cfg(test)]
extern crate rand_chacha;

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
pub mod packwriter;
pub mod repack;
pub mod uniondatastore;
pub mod unionhistorystore;

pub use datapack::{DataEntry, DataPack, DataPackVersion};
pub use datastore::{DataStore, Delta, Metadata};
pub use historypack::{HistoryEntry, HistoryPack, HistoryPackVersion};
pub use historystore::{Ancestors, HistoryStore, NodeInfo};
pub use mutabledatapack::MutableDataPack;
pub use mutablehistorypack::MutableHistoryPack;
