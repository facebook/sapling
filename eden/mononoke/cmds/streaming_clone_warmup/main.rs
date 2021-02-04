/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, Error};
use blobstore::Blobstore;
use blobstore_factory::{make_blobstore, BlobstoreOptions, ReadOnlyStorage};
use cacheblob::new_memcache_blobstore;
use cached_config::ConfigStore;
use clap::Arg;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{self, try_join, try_join_all, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use slog::{error, info};
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::{myrouter_ready, MysqlOptions};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use streaming_clone::{RevlogStreamingChunks, SqlStreamingChunksFetcher};
use tokio::time;

const REPO_ARG: &str = "repo";
const PERIOD_ARG: &str = "warmup-period";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Utility to keep streaming clone data warm")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .about("Utility to keep streaming clone data warm")
        .arg(
            Arg::with_name(REPO_ARG)
                .long(REPO_ARG)
                .takes_value(true)
                .required(true)
                .multiple(true)
                .help("Repository name to warm-up"),
        )
        .arg(
            Arg::with_name(PERIOD_ARG)
                .long(PERIOD_ARG)
                .takes_value(true)
                .required(false)
                .default_value("900")
                .help("Period of warmup runs in secods"),
        );
    let matches = app.get_matches();

    let logger = args::init_logging(fb, &matches)?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    args::init_config_store(fb, &logger, &matches)?;
    helpers::block_execute(
        run(ctx, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| "streaming_clone_warmup".to_string()),
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run<'a>(ctx: CoreContext, matches: &'a MononokeMatches<'a>) -> Result<(), Error> {
    let period_secs: u64 = matches
        .value_of(PERIOD_ARG)
        .ok_or_else(|| anyhow!("--{} argument is required", PERIOD_ARG))?
        .parse()?;
    let period = Duration::from_secs(period_secs);

    let reponames: Vec<_> = matches
        .values_of(REPO_ARG)
        .ok_or_else(|| anyhow!("--{} argument is required", REPO_ARG))?
        .map(ToString::to_string)
        .collect();
    if reponames.is_empty() {
        error!(ctx.logger(), "At least one repo had to be specified");
        return Ok(());
    }

    let config_store = args::init_config_store(ctx.fb, ctx.logger(), matches)?;
    let mysql_options = args::parse_mysql_options(matches);
    let blobstore_options = args::parse_blobstore_options(matches)?;
    let configs = args::load_repo_configs(config_store, matches)?;

    // wait for myrouter
    myrouter_ready(
        Some("xdb.mononoke_production".to_string()),
        &mysql_options,
        ctx.logger().clone(),
    )
    .compat()
    .await?;

    let mut warmers = Vec::new();
    for reponame in reponames {
        let config = configs
            .repos
            .get(&reponame)
            .ok_or_else(|| anyhow!("unknown repository: {}", reponame))?;
        let warmer = StreamingCloneWarmup::new(
            ctx.clone(),
            reponame,
            config,
            &mysql_options,
            blobstore_options.clone(),
            config_store,
        )
        .await?;
        warmers.push(warmer);
    }

    let offset_delay = period / warmers.len() as u32;
    let mut tasks = Vec::new();
    for (index, warmer) in warmers.into_iter().enumerate() {
        let ctx = ctx.clone();
        tasks.push(async move {
            // spread fetches over period, to reduce memory consumption
            time::delay_for(offset_delay * index as u32).await;
            warmer.warmer_task(ctx.clone(), period).await?;
            Ok::<_, Error>(())
        });
    }
    try_join_all(tasks).await?;
    Ok(())
}

struct StreamingCloneWarmup {
    fetcher: SqlStreamingChunksFetcher,
    blobstore: Arc<dyn Blobstore>,
    repoid: RepositoryId,
    reponame: String,
}

impl StreamingCloneWarmup {
    async fn new(
        ctx: CoreContext,
        reponame: String,
        config: &RepoConfig,
        mysql_options: &MysqlOptions,
        blobstore_options: BlobstoreOptions,
        config_store: &ConfigStore,
    ) -> Result<Self, Error> {
        // Create blobstore that contains streaming clone chunks, without cachelib
        // layer (we want to hit memcache even if it is available in cachelib), and
        // with memcache layer identical to production setup.
        let blobstore = make_blobstore(
            ctx.fb,
            config.storage_config.blobstore.clone(),
            mysql_options,
            ReadOnlyStorage(true),
            &blobstore_options,
            ctx.logger(),
            config_store,
        )
        .await?;
        let blobstore = new_memcache_blobstore(ctx.fb, blobstore, "multiplexed", "")?;
        let blobstore = PrefixBlobstore::new(blobstore, config.repoid.prefix());

        let fetcher = SqlStreamingChunksFetcher::with_metadata_database_config(
            ctx.fb,
            &config.storage_config.metadata,
            mysql_options,
            true, /*read-only*/
        )
        .await?;

        Ok(Self {
            fetcher,
            blobstore: Arc::new(blobstore),
            repoid: config.repoid,
            reponame,
        })
    }

    /// Periodically fetch streaming clone data
    async fn warmer_task(&self, ctx: CoreContext, period: Duration) -> Result<(), Error> {
        info!(ctx.logger(), "[{}] warmer started", self.reponame);
        loop {
            let start = Instant::now();
            let chunks = self
                .fetcher
                .fetch_changelog(ctx.clone(), self.repoid, self.blobstore.clone())
                .compat()
                .await?;
            info!(
                ctx.logger(),
                "[{}] index fetched in: {:.2?}",
                self.reponame,
                start.elapsed()
            );

            let size = chunks_warmup(ctx.clone(), chunks).await? as f32;
            let duration = start.elapsed();
            info!(
                ctx.logger(),
                "[{}] fetching complete in: time:{:.2?} speed:{:.1?} b/s size: {}",
                self.reponame,
                duration,
                size / duration.as_secs_f32(),
                size,
            );

            // sleep if needed
            if duration < period {
                let delay = period - duration;
                info!(
                    ctx.logger(),
                    "[{}] sleeping for: {:?}", self.reponame, delay
                );
                time::delay_for(delay).await;
            }
        }
    }
}

async fn chunks_warmup(ctx: CoreContext, chunks: RevlogStreamingChunks) -> Result<usize, Error> {
    let RevlogStreamingChunks {
        index_blobs,
        data_blobs,
        index_size: index_size_expected,
        data_size: data_size_expected,
    } = chunks;

    let index = stream::iter(
        index_blobs
            .into_iter()
            .map(|f| f.compat().map_ok(|b| b.len())),
    )
    .buffer_unordered(100)
    .try_fold(0usize, |acc, size| future::ok(acc + size));

    let data = stream::iter(
        data_blobs
            .into_iter()
            .map(|f| f.compat().map_ok(|b| b.len())),
    )
    .buffer_unordered(100)
    .try_fold(0usize, |acc, size| future::ok(acc + size));

    let (index_size, data_size) = try_join(index, data).await?;
    if index_size_expected != index_size {
        error!(
            ctx.logger(),
            "incorrect index size: expected:{} received:{}", index_size_expected, index_size
        );
    }
    if data_size_expected != data_size {
        error!(
            ctx.logger(),
            "incorrect data size: expected:{} received:{}", data_size_expected, data_size
        );
    }
    Ok(index_size + data_size)
}
