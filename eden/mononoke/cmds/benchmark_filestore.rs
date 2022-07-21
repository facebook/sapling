/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]

use anyhow::format_err;
use anyhow::Error;
use blobstore::Blobstore;
use blobstore_factory::make_sql_blobstore_xdb;
use blobstore_factory::ReadOnlyStorage;
use bytes::Bytes;
use bytes::BytesMut;
use cacheblob::new_memcache_blobstore_no_lease;
use cached_config::ConfigStore;
use clap_old::Arg;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use mononoke_types::BlobstoreKey;
use mononoke_types::ContentMetadata;
use rand::Rng;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use throttledblob::ThrottledBlob;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

const NAME: &str = "benchmark_filestore";

const CMD_MANIFOLD: &str = "manifold";
const CMD_MEMORY: &str = "memory";
const CMD_XDB: &str = "xdb";

const ARG_MANIFOLD_BUCKET: &str = "manifold-bucket";
const ARG_SHARDMAP: &str = "shardmap";
const ARG_SHARD_COUNT: &str = "shard-count";
const ARG_INPUT_CAPACITY: &str = "input-capacity";
const ARG_CHUNK_SIZE: &str = "chunk-size";
const ARG_CONCURRENCY: &str = "concurrency";
const ARG_MEMCACHE: &str = "memcache";
const ARG_CACHELIB_SIZE: &str = "cachelib-size";
const ARG_INPUT: &str = "input";
const ARG_DELAY: &str = "delay";
const ARG_RANDOMIZE: &str = "randomize";
const ARG_READ_COUNT: &str = "read-count";

fn log_perf<I, E: Debug>(stats: FutureStats, res: &Result<I, E>, len: u64) {
    match res {
        Ok(_) => {
            let bytes_per_ns = (len as f64) / (stats.completion_time.as_nanos() as f64);
            let mbytes_per_s = bytes_per_ns * (10_u128.pow(9) as f64) / (2_u128.pow(20) as f64);
            let gb_per_s = mbytes_per_s * 8_f64 / 1024_f64;
            eprintln!(
                "Success: {:.2} MB/s ({:.2} Gb/s) ({:?})",
                mbytes_per_s, gb_per_s, stats
            );
        }
        Err(e) => {
            eprintln!("Failure: {:?}", e);
        }
    };
}

async fn read<B: Blobstore>(
    blob: &B,
    ctx: &CoreContext,
    content_metadata: &ContentMetadata,
) -> Result<(), Error> {
    let key = FetchKey::Canonical(content_metadata.content_id);
    eprintln!(
        "Fetch start: {:?} ({:?} B)",
        key, content_metadata.total_size
    );

    let stream = filestore::fetch(blob, ctx.clone(), &key)
        .await?
        .ok_or_else(|| format_err!("Fetch failed: no stream"))?;

    let (stats, res) = stream.try_for_each(|_| async { Ok(()) }).timed().await;
    log_perf(stats, &res, content_metadata.total_size);

    // ignore errors - all we do is log them in `log_perf`
    match res {
        Ok(_) => Ok(()),
        Err(_) => Ok(()),
    }
}

async fn run_benchmark_filestore<'a>(
    ctx: &'a CoreContext,
    matches: &'a MononokeMatches<'a>,
    blob: Arc<dyn Blobstore>,
) -> Result<(), Error> {
    let input = matches.value_of(ARG_INPUT).unwrap().to_string();

    let input_capacity: usize = matches.value_of(ARG_INPUT_CAPACITY).unwrap().parse()?;

    let chunk_size: u64 = matches.value_of(ARG_CHUNK_SIZE).unwrap().parse()?;

    let concurrency: usize = matches.value_of(ARG_CONCURRENCY).unwrap().parse()?;

    let read_count: usize = matches.value_of(ARG_READ_COUNT).unwrap().parse()?;

    let delay: Option<Duration> = matches
        .value_of(ARG_DELAY)
        .map(|seconds| -> Result<Duration, Error> {
            let seconds = seconds.parse().map_err(Error::from)?;
            Ok(Duration::new(seconds, 0))
        })
        .transpose()?;

    let randomize = matches.is_present(ARG_RANDOMIZE);

    let config = FilestoreConfig {
        chunk_size: Some(chunk_size),
        concurrency,
    };

    eprintln!("Test with {:?}, writing into {:?}", config, blob);

    let file = File::open(input).await?;
    let metadata = file.metadata().await?;

    let data = BufReader::with_capacity(input_capacity, file);
    let data = FramedRead::new(data, BytesCodec::new()).map_ok(BytesMut::freeze);
    let len = metadata.len();

    let (len, data) = if randomize {
        let bytes = rand::thread_rng().gen::<[u8; 32]>();
        let bytes = Bytes::copy_from_slice(&bytes[..]);
        (
            len + (bytes.len() as u64),
            stream::iter(vec![Ok(bytes)]).chain(data).left_stream(),
        )
    } else {
        (len, data.right_stream())
    };

    eprintln!("Write start: {:?} B", len);

    let req = StoreRequest::new(len);

    let (stats, res) = filestore::store(&blob, config, ctx, &req, data.map_err(Error::from))
        .timed()
        .await;
    log_perf(stats, &res, len);

    let metadata = res?;

    match delay {
        Some(delay) => {
            tokio_shim::time::sleep(delay).await;
        }
        None => {}
    }

    eprintln!("Write committed: {:?}", metadata.content_id.blobstore_key());

    for _c in 0..read_count {
        read(&blob, ctx, &metadata).await?;
    }

    Ok(())
}

#[cfg(fbcode_build)]
const TEST_DATA_TTL: Option<Duration> = Some(Duration::from_secs(3600));

async fn get_blob<'a>(
    fb: FacebookInit,
    matches: &'a MononokeMatches<'a>,
    config_store: &ConfigStore,
) -> Result<Arc<dyn Blobstore>, Error> {
    let blobstore_options = matches.blobstore_options();
    let readonly_storage = matches.readonly_storage();
    let blob: Arc<dyn Blobstore> = match matches.subcommand() {
        (CMD_MANIFOLD, Some(sub)) => {
            #[cfg(fbcode_build)]
            {
                use manifoldblob::ManifoldBlob;
                use prefixblob::PrefixBlobstore;

                let bucket = sub.value_of(ARG_MANIFOLD_BUCKET).unwrap();
                let put_behaviour = blobstore_options.put_behaviour;
                let manifold = ManifoldBlob::new(
                    fb,
                    bucket,
                    TEST_DATA_TTL,
                    blobstore_options.manifold_options.api_key.as_deref(),
                    blobstore_options.manifold_options.weak_consistency_ms,
                    blobstore_options.manifold_options.request_priority,
                    put_behaviour,
                )
                .map_err(|e| -> Error { e })?;
                let blobstore = PrefixBlobstore::new(manifold, format!("{}.", NAME));
                Arc::new(blobstore)
            }
            #[cfg(not(fbcode_build))]
            {
                let _ = sub;
                unimplemented!("Accessing Manifold is not implemented in non fbcode builds");
            }
        }
        (CMD_MEMORY, Some(_)) => Arc::new(memblob::Memblob::default()),
        (CMD_XDB, Some(sub)) => {
            let shardmap_or_tier = sub.value_of(ARG_SHARDMAP).unwrap().to_string();
            let shard_count = sub
                .value_of(ARG_SHARD_COUNT)
                .map(|v| v.parse())
                .transpose()?;
            let blobstore = make_sql_blobstore_xdb(
                fb,
                shardmap_or_tier,
                shard_count,
                blobstore_options,
                *readonly_storage,
                blobstore_options.put_behaviour,
                config_store,
            )
            .await?;
            Arc::new(blobstore)
        }
        _ => unreachable!(),
    };

    let blob: Arc<dyn Blobstore> = if matches.is_present(ARG_MEMCACHE) {
        Arc::new(new_memcache_blobstore_no_lease(fb, blob, NAME, "")?)
    } else {
        blob
    };

    let blob: Arc<dyn Blobstore> = match matches.value_of(ARG_CACHELIB_SIZE) {
        Some(size) => {
            #[cfg(fbcode_build)]
            {
                let cache_size_bytes = size.parse()?;
                cachelib::init_cache(fb, cachelib::LruCacheConfig::new(cache_size_bytes))?;

                let presence_pool = cachelib::get_or_create_pool(
                    "presence",
                    cachelib::get_available_space()? / 20,
                )?;
                let blob_pool =
                    cachelib::get_or_create_pool("blobs", cachelib::get_available_space()?)?;

                Arc::new(cacheblob::new_cachelib_blobstore_no_lease(
                    blob,
                    Arc::new(blob_pool),
                    Arc::new(presence_pool),
                    blobstore_options.cachelib_options,
                ))
            }
            #[cfg(not(fbcode_build))]
            {
                let _ = size;
                unimplemented!("Using cachelib is not implemented for non fbcode build");
            }
        }
        None => blob,
    };

    // ThrottledBlob is a noop if no throttling requested
    let blob = Arc::new(ThrottledBlob::new(blob, blobstore_options.throttle_options).await);

    Ok(blob)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let manifold_subcommand = SubCommand::with_name(CMD_MANIFOLD).arg(
        Arg::with_name(ARG_MANIFOLD_BUCKET)
            .takes_value(true)
            .required(false),
    );

    let memory_subcommand = SubCommand::with_name(CMD_MEMORY);
    let xdb_subcommand = SubCommand::with_name(CMD_XDB)
        .arg(
            Arg::with_name(ARG_SHARDMAP)
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_SHARD_COUNT)
                .long(ARG_SHARD_COUNT)
                .takes_value(true)
                .required(false),
        );

    let app = args::MononokeAppBuilder::new(NAME)
        .with_all_repos()
        .with_readonly_storage_default(ReadOnlyStorage(false))
        .build()
        .arg(
            Arg::with_name(ARG_INPUT_CAPACITY)
                .long(ARG_INPUT_CAPACITY)
                .takes_value(true)
                .required(false)
                .default_value("8192"),
        )
        .arg(
            Arg::with_name(ARG_CHUNK_SIZE)
                .long(ARG_CHUNK_SIZE)
                .takes_value(true)
                .required(false)
                .default_value("1048576"),
        )
        .arg(
            Arg::with_name(ARG_CONCURRENCY)
                .long(ARG_CONCURRENCY)
                .takes_value(true)
                .required(false)
                .default_value("1"),
        )
        .arg(
            Arg::with_name(ARG_MEMCACHE)
                .long(ARG_MEMCACHE)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_CACHELIB_SIZE)
                .long(ARG_CACHELIB_SIZE)
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_DELAY)
                .long(ARG_DELAY)
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_RANDOMIZE)
                .long(ARG_RANDOMIZE)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_READ_COUNT)
                .long(ARG_READ_COUNT)
                .takes_value(true)
                .default_value("2")
                .required(true),
        )
        .arg(Arg::with_name(ARG_INPUT).takes_value(true).required(true))
        .subcommand(manifold_subcommand)
        .subcommand(memory_subcommand)
        .subcommand(xdb_subcommand);

    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let config_store = matches.config_store();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let runtime = tokio::runtime::Runtime::new().map_err(Error::from)?;

    let blob = runtime.block_on(get_blob(fb, &matches, config_store))?;

    runtime.block_on(run_benchmark_filestore(&ctx, &matches, blob))?;

    Ok(())
}
