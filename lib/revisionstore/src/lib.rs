// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

extern crate byteorder;
extern crate crypto;
#[macro_use]
extern crate failure;
extern crate memmap;
extern crate pylz4;
extern crate tempfile;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;

mod dataindex;
mod fanouttable;
mod unionstore;

pub mod datapack;
pub mod datastore;
pub mod error;
pub mod historystore;
pub mod key;
pub mod loosefile;
pub mod mutabledatapack;
pub mod node;
pub mod repack;
pub mod uniondatastore;
pub mod unionhistorystore;
