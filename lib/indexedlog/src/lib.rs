// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(dead_code)]

//! # Indexed Log
//!
//! Indexed Log provides an integrity-checked, append-only storage
//! with index support.
//!
//! See [log::Log] for the main structure. The index and integrity
//! check parts can be used independently. See [index::Index] and
//! [checksum_table::ChecksumTable] for details.

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

#[macro_use]
mod macros;

pub mod base16;
pub mod checksum_table;
pub mod errors;
pub mod index;
pub mod lock;
pub mod log;
pub mod rotate;
pub mod utils;
