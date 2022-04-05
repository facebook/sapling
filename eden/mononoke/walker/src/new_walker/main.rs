/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(process_exitcode_placeholder)]
#![feature(async_closure)]

mod args;

use anyhow::Error;
use blobstore_factory::{BlobstoreArgDefaults, ReadOnlyStorage};
use clap::Parser;
use fbinit::FacebookInit;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::{MononokeApp, MononokeAppBuilder};

use cmdlib::args::CachelibSettings;
use cmdlib_scrubbing::ScrubAppExtension;

use multiplexedblob::ScrubWriteMostly;
use std::num::NonZeroU32;

#[derive(Parser)]
struct WalkerArgs {}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    // FIXME: Investigate why some SQL queries kicked off by the walker take 30s or more.
    newfilenodes::disable_sql_timeouts();

    let cachelib_defaults = CachelibSettings {
        cache_size: 2 * 1024 * 1024 * 1024,
        blobstore_cachelib_only: true,
        ..Default::default()
    };

    let blobstore_defaults = BlobstoreArgDefaults {
        read_qps: NonZeroU32::new(20000),
        cachelib_attempt_zstd: Some(false),
        ..Default::default()
    };

    let scrub_extension = ScrubAppExtension {
        write_mostly_missing: Some(ScrubWriteMostly::SkipMissing),
        ..Default::default()
    };

    let read_only_storage = ReadOnlyStorage(true);

    MononokeAppBuilder::new(fb)
        .with_app_extension(scrub_extension)
        .with_default_cachelib_settings(cachelib_defaults)
        .with_arg_defaults(blobstore_defaults)
        .with_arg_defaults(read_only_storage)
        .with_app_extension(Fb303AppExtension {})
        .build::<WalkerArgs>()?
        .run(async_main)
}

async fn async_main(_app: MononokeApp) -> Result<(), Error> {
    Ok(())
}
