/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod blobstore;
#[cfg(fbcode_build)]
mod facebook;
mod sql;

pub use ::blobstore::{PutBehaviour, DEFAULT_PUT_BEHAVIOUR};
pub use blobstore_stats::OperationType;
pub use cacheblob::CachelibBlobstoreOptions;
pub use chaosblob::ChaosOptions;
pub use delayblob::DelayOptions;
#[cfg(fbcode_build)]
pub use facebook::ManifoldOptions;
pub use multiplexedblob::{
    scrub::{default_scrub_handler, ScrubOptions, ScrubWriteMostly},
    ScrubAction, ScrubHandler,
};
pub use packblob::PackOptions;
pub use samplingblob::ComponentSamplingHandler;
pub use throttledblob::ThrottleOptions;

pub use crate::blobstore::{
    make_blobstore, make_packblob, make_sql_blobstore, make_sql_blobstore_xdb, BlobstoreOptions,
};
pub use crate::sql::{make_metadata_sql_factory, MetadataSqlFactory, SqlTierInfo};

#[derive(Copy, Clone, PartialEq)]
pub struct ReadOnlyStorage(pub bool);
