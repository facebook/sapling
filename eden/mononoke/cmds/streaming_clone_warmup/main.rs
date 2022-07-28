/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobstore_factory::make_blobstore;
use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use cacheblob::new_memcache_blobstore;
use cached_config::ConfigStore;
use clap_old::Arg;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_types::RepoConfig;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::error;
use slog::info;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use streaming_clone::RevlogStreamingChunks;
use streaming_clone::StreamingClone;
use streaming_clone::StreamingCloneBuilder;

use tokio::time;

const REPO_ARG: &str = "repo";
const REPO_WITH_TAGS_ARG: &str = "repo-with-tags";
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
                .required(false)
                .multiple(true)
                .help("Repository name to warm-up, and empty tag is assumed"),
        )
        .arg(
            Arg::with_name(REPO_WITH_TAGS_ARG)
                .long(REPO_WITH_TAGS_ARG)
                .takes_value(true)
                .required(false)
                .multiple(true)
                .help("Repository name with a list of tags to warmup in format REPO=tag1,tag2."),
        )
        .arg(
            Arg::with_name(PERIOD_ARG)
                .long(PERIOD_ARG)
                .takes_value(true)
                .required(false)
                .default_value("900")
                .help("Period of warmup runs in secods"),
        );
    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    helpers::block_execute(
        run(ctx, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| "streaming_clone_warmup".to_string()),
        logger,
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

    let mut reponames_with_tags = vec![];
    if let Some(values) = matches.values_of(REPO_ARG) {
        // Assume empty tag
        reponames_with_tags.extend(
            values
                .map(ToString::to_string)
                .map(|reponame| (reponame, None)),
        );
    }

    if let Some(values) = matches.values_of(REPO_WITH_TAGS_ARG) {
        for value in values {
            let (reponame, tags) = split_repo_with_tags(value)?;
            for tag in tags {
                reponames_with_tags.push((reponame.clone(), Some(tag)));
            }
        }
    }

    if reponames_with_tags.is_empty() {
        error!(ctx.logger(), "At least one repo had to be specified");
        return Ok(());
    }

    let config_store = matches.config_store();
    let mysql_options = matches.mysql_options();
    let blobstore_options = matches.blobstore_options();
    let configs = args::load_repo_configs(config_store, matches)?;

    let mut warmers = Vec::new();
    for (reponame, tag) in reponames_with_tags {
        let config = configs
            .repos
            .get(&reponame)
            .ok_or_else(|| anyhow!("unknown repository: {}", reponame))?;
        let warmer = StreamingCloneWarmup::new(
            ctx.clone(),
            reponame,
            tag,
            config,
            mysql_options,
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
            time::sleep(offset_delay * index as u32).await;
            warmer.warmer_task(ctx.clone(), period).await?;
            Ok::<_, Error>(())
        });
    }
    try_join_all(tasks).await?;
    Ok(())
}

fn split_repo_with_tags(s: &str) -> Result<(String, Vec<String>), Error> {
    if let Some((reponame, tags)) = s.split_once('=') {
        let tags = tags.split(',').map(|s| s.to_string()).collect();

        Ok((reponame.to_string(), tags))
    } else {
        Err(anyhow!("invalid format for repo with tags: {}", s))
    }
}

struct StreamingCloneWarmup {
    streaming_clone: StreamingClone,
    reponame: String,
    tag: Option<String>,
}

impl StreamingCloneWarmup {
    async fn new(
        ctx: CoreContext,
        reponame: String,
        tag: Option<String>,
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
            &blobstore_factory::default_scrub_handler(),
            None,
        )
        .await?;
        let blobstore = new_memcache_blobstore(ctx.fb, blobstore, "multiplexed", "")?;
        let repo_blobstore = Arc::new(RepoBlobstore::new(
            blobstore,
            None,
            config.repoid,
            MononokeScubaSampleBuilder::with_discard(),
        ));

        // Because we want to use our custom blobstore, we must construct the
        // streaming clone attribute directly.
        let streaming_clone = StreamingCloneBuilder::with_metadata_database_config(
            ctx.fb,
            &config.storage_config.metadata,
            mysql_options,
            true, /*read-only*/
        )?
        .build(config.repoid, repo_blobstore);

        Ok(Self {
            streaming_clone,
            reponame,
            tag,
        })
    }

    /// Periodically fetch streaming clone data
    async fn warmer_task(&self, ctx: CoreContext, period: Duration) -> Result<(), Error> {
        if let Some(ref tag) = self.tag {
            info!(ctx.logger(), "[{}:{}] warmer started", self.reponame, tag);
        } else {
            info!(ctx.logger(), "[{}] warmer started", self.reponame);
        };

        loop {
            let tag = None;
            let start = Instant::now();
            let chunks = self
                .streaming_clone
                .fetch_changelog(ctx.clone(), tag)
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
                time::sleep(delay).await;
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

    let index = stream::iter(index_blobs.into_iter().map(|f| f.map_ok(|b| b.len())))
        .buffer_unordered(100)
        .try_fold(0usize, |acc, size| future::ok(acc + size));

    let data = stream::iter(data_blobs.into_iter().map(|f| f.map_ok(|b| b.len())))
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
