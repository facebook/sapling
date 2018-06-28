// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

#[macro_use]
extern crate failure;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

mod unionstore;

pub mod datastore;
pub mod error;
pub mod historystore;
pub mod key;
pub mod node;
pub mod uniondatastore;
