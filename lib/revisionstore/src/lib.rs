// Copyright Facebook, Inc. 2018
//! revisionstore - Data and history store for generic revision data (usually commit, manifest,
//! and file data)

#[macro_use]
extern crate failure;

mod unionstore;

pub mod error;
pub mod datastore;
pub mod key;
pub mod node;
