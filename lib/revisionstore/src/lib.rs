// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

extern crate byteorder;
#[cfg(not(fbcode_build))]
extern crate crypto;
#[macro_use]
extern crate failure;
extern crate lz4_pyframe;
extern crate memmap;
#[cfg(fbcode_build)]
extern crate rust_crypto as crypto;
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
pub mod uniondatastore;
pub mod unionhistorystore;
