/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use clap::Args;

mod args;
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
pub use facebook::{ManifoldArgs, ManifoldOptions};
pub use multiplexedblob::{
    scrub::{default_scrub_handler, ScrubOptions, ScrubWriteMostly},
    ScrubAction, ScrubHandler,
};
pub use packblob::PackOptions;
pub use samplingblob::ComponentSamplingHandler;
pub use throttledblob::ThrottleOptions;

pub use crate::args::BlobstoreArgs;
pub use crate::blobstore::{
    make_blobstore, make_packblob, make_sql_blobstore, make_sql_blobstore_xdb, BlobstoreOptions,
};
pub use crate::sql::{make_metadata_sql_factory, MetadataSqlFactory, SqlTierInfo};

#[derive(Copy, Clone, PartialEq)]
pub struct ReadOnlyStorage(pub bool);

/// Command line arguments for controlling read-only storage
#[derive(Args, Debug)]
pub struct ReadOnlyStorageArgs {
    /// Error on any attempts to write to storage if set to true,
    /// allow writes to storaga if set to false (overrides app default)
    // This is Option<bool> as we distinguish between option being
    // not present vs being set to false.
    #[clap(long, value_name = "BOOL")]
    pub with_readonly_storage: Option<bool>,
}
