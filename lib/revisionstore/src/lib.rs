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

#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;

mod ancestors;
mod dataindex;
mod historyindex;
mod fanouttable;
mod unionstore;

pub mod datapack;
pub mod datastore;
pub mod error;
pub mod historypack;
pub mod historystore;
pub mod key;
pub mod loosefile;
pub mod mutabledatapack;
pub mod mutablehistorypack;
pub mod node;
pub mod repack;
pub mod uniondatastore;
pub mod unionhistorystore;
