/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]

use arg_extensions::ArgDefaults;
use clap::Args;

mod args;
mod blobstore;
#[cfg(fbcode_build)]
mod facebook;
mod sql;

pub use ::blobstore::PutBehaviour;
pub use ::blobstore::DEFAULT_PUT_BEHAVIOUR;
pub use blobstore_stats::OperationType;
pub use cacheblob::CachelibBlobstoreOptions;
pub use chaosblob::ChaosOptions;
pub use delayblob::DelayOptions;
#[cfg(fbcode_build)]
pub use facebook::ManifoldArgs;
#[cfg(fbcode_build)]
pub use facebook::ManifoldOptions;
pub use multiplexedblob::scrub::default_scrub_handler;
pub use multiplexedblob::scrub::ScrubOptions;
pub use multiplexedblob::scrub::SrubWriteOnly;
pub use multiplexedblob::ScrubAction;
pub use multiplexedblob::ScrubHandler;
pub use packblob::PackOptions;
pub use samplingblob::ComponentSamplingHandler;
pub use throttledblob::ThrottleOptions;

pub use crate::args::BlobstoreArgDefaults;
pub use crate::args::BlobstoreArgs;
pub use crate::blobstore::make_blobstore;
pub use crate::blobstore::make_blobstore_enumerable_with_unlink;
pub use crate::blobstore::make_packblob;
pub use crate::blobstore::make_sql_blobstore;
pub use crate::blobstore::make_sql_blobstore_xdb;
pub use crate::blobstore::BlobstoreOptions;
pub use crate::sql::make_metadata_sql_factory;
pub use crate::sql::MetadataSqlFactory;
pub use crate::sql::SqlTierInfo;

#[derive(Copy, Clone, PartialEq)]
pub struct ReadOnlyStorage(pub bool);

impl ArgDefaults for ReadOnlyStorage {
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        vec![("with-readonly-storage", self.0.to_string())]
    }
}

impl ReadOnlyStorage {
    pub fn from_args(args: &ReadOnlyStorageArgs) -> Self {
        ReadOnlyStorage(args.with_readonly_storage)
    }
}

/// Command line arguments for controlling read-only storage
#[derive(Args, Debug)]
pub struct ReadOnlyStorageArgs {
    /// Error on any attempts to write to storage if set to true
    // For compatibility with existing usage, allows usage as
    // `--with-readonly-storage=true`.
    #[clap(
        long,
        value_name = "BOOL",
        parse(try_from_str),
        default_value = "false",
        default_missing_value = "true"
    )]
    pub with_readonly_storage: bool,
}
