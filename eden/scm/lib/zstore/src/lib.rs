/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Blob store backed by zstd delta compression.

mod errors;
mod zstore;

pub use crate::zstore::{sha1, Id20, Zstore};
pub use errors::{Error, Result};
pub use indexedlog::Repair;
