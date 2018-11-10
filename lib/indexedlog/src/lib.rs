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

extern crate atomicwrites;
extern crate byteorder;
extern crate fs2;
extern crate memmap;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate rand;
#[cfg(test)]
extern crate tempdir;
extern crate twox_hash;
extern crate vlqencoding;

pub mod base16;
pub mod checksum_table;
pub mod index;
mod lock;
pub mod log;
mod utils;
