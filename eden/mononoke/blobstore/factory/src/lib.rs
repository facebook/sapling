/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod blobstore;
#[cfg(fbcode_build)]
mod facebook;
mod sql;

pub use cacheblob::CachelibBlobstoreOptions;
pub use chaosblob::ChaosOptions;
pub use packblob::PackOptions;
pub use throttledblob::ThrottleOptions;

pub use crate::blobstore::{make_blobstore, make_blobstore_multiplexed, BlobstoreOptions};
pub use crate::sql::{make_metadata_sql_factory, MetadataSqlFactory};

#[derive(Copy, Clone, PartialEq)]
pub struct ReadOnlyStorage(pub bool);

#[derive(Copy, Clone, PartialEq)]
pub enum Scrubbing {
    Enabled,
    Disabled,
}
