/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

mod args;
mod commands;
mod detail;
mod setup;

use std::num::NonZeroU32;

use anyhow::Error;
use blobstore_factory::BlobstoreArgDefaults;
use blobstore_factory::ReadOnlyStorage;
use clap::ArgGroup;
use clap::Parser;
use cmdlib::args::CachelibSettings;
use cmdlib_scrubbing::ScrubAppExtension;
use fbinit::FacebookInit;
use metaconfig_types::WalkerJobType;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::fb303::ReadyFlagService;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use multiplexedblob::SrubWriteOnly;

#[derive(Parser)]
#[clap(group(
    ArgGroup::new("walkerargs")
        .required(true)
        .multiple(true)
        .args(&["repo-id", "repo-name", "sharded-service-name", "walker-type"]),
))]
struct WalkerArgs {
    /// List of Repo IDs or Repo Names used when sharded-service-name
    /// is absent.
    #[clap(flatten)]
    pub repos: MultiRepoArgs,

    /// The name of ShardManager service to be used when the walker
    /// functionality is desired to be executed in a sharded setting.
    #[clap(long, conflicts_with = "multirepos", requires = "walker-type")]
    pub sharded_service_name: Option<String>,

    /// The type of the walker job that needs to run for the current
    /// repo.
    #[clap(arg_enum, long, conflicts_with = "multirepos")]
    pub walker_type: Option<WalkerJobType>,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    // FIXME: Investigate why some SQL queries kicked off by the walker take 30s or more.
    newfilenodes::disable_sql_timeouts();

    let service = ReadyFlagService::new();

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
        write_only_missing: Some(SrubWriteOnly::SkipMissing),
        ..Default::default()
    };

    let read_only_storage = ReadOnlyStorage(true);

    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(scrub_extension)
        .with_default_cachelib_settings(cachelib_defaults)
        .with_arg_defaults(blobstore_defaults)
        .with_arg_defaults(read_only_storage)
        .with_app_extension(Fb303AppExtension {})
        .build_with_subcommands::<WalkerArgs>(subcommands)?;

    // TODO: we may want to set_ready after the repo setup is done
    service.set_ready();

    app.run_with_monitoring_and_logging(async_main, "walker", service)
}

async fn async_main(app: MononokeApp) -> Result<(), Error> {
    commands::dispatch(app).await
}
