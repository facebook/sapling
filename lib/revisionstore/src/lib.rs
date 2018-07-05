// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

extern crate byteorder;
extern crate crypto;
#[macro_use]
extern crate failure;
extern crate lz4_pyframe;
extern crate memmap;
extern crate rand;
extern crate tempfile;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

mod dataindex;
mod fanouttable;
mod mutabledatapack;
mod unionstore;

pub mod datapack;
pub mod datastore;
pub mod error;
pub mod historystore;
pub mod key;
pub mod node;
pub mod uniondatastore;
pub mod unionhistorystore;
