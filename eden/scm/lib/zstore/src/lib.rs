/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Blob store backed by zstd delta compression.

mod errors;
mod zstore;

pub use errors::Error;
pub use errors::Result;
pub use indexedlog::Repair;

pub use crate::zstore::sha1;
pub use crate::zstore::Id20;
pub use crate::zstore::OpenOptions;
pub use crate::zstore::Zstore;
