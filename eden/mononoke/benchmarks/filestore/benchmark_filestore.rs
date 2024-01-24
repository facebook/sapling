/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]

use std::fmt::Debug;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::format_err;
use anyhow::Error;
use blobstore::Blobstore;
use blobstore_factory::make_sql_blobstore_xdb;
use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use bytes::Bytes;
use bytes::BytesMut;
use cacheblob::new_memcache_blobstore;
use cached_config::ConfigStore;
use clap::Parser;
use clap::ValueEnum;
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
use mononoke_app::MononokeAppBuilder;
use mononoke_types::BlobstoreKey;
use mononoke_types::ContentMetadataV2;
use rand::Rng;
use throttledblob::ThrottledBlob;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

const NAME: &str = "benchmark_filestore";

/// Benchmark the filestore
#[derive(Parser)]
struct BenchmarkArgs {
    #[clap(long, default_value_t = 8192)]
    input_capacity: usize,

    #[clap(long, default_value_t = 1048576)]
    chunk_size: u64,

    #[clap(long, default_value_t = 1)]
    concurrency: usize,

    #[clap(long)]
    memcache: bool,

    #[clap(long)]
    cachelib_size: Option<usize>,

    #[clap(long)]
    delay: Option<u64>,

    #[clap(long)]
    randomize: bool,

    #[clap(long, default_value_t = 2)]
    read_count: usize,

    #[clap(long, required_if_eq("blobstore_type", "manifold"))]
    manifold_bucket: Option<String>,

    #[clap(long, required_if_eq("blobstore_type", "xdb"))]
    shardmap: Option<String>,

    #[clap(long)]
    shard_count: Option<NonZeroUsize>,

    /// Data file to use as input.
    input: PathBuf,

    /// Which type of blobstore to benchmark against.
    #[clap(value_enum)]
    blobstore_type: BlobstoreType,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum BlobstoreType {
    Manifold,
    Memory,
    Xdb,
}

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
    blobstore: &B,
    ctx: &CoreContext,
    content_metadata: &ContentMetadataV2,
) -> Result<(), Error> {
    let key = FetchKey::Canonical(content_metadata.content_id);
    eprintln!(
        "Fetch start: {:?} ({:?} B)",
        key, content_metadata.total_size
    );

    let stream = filestore::fetch(blobstore, ctx.clone(), &key)
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
    args: &BenchmarkArgs,
    blobstore: Arc<dyn Blobstore>,
) -> Result<(), Error> {
    let config = FilestoreConfig {
        chunk_size: Some(args.chunk_size),
        concurrency: args.concurrency,
    };

    eprintln!("Test with {:?}, writing into {:?}", config, blobstore);

    let file = File::open(&args.input).await?;
    let metadata = file.metadata().await?;

    let data = BufReader::with_capacity(args.input_capacity, file);
    let data = FramedRead::new(data, BytesCodec::new()).map_ok(BytesMut::freeze);
    let len = metadata.len();

    let (len, data) = if args.randomize {
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

    let (stats, res) = filestore::store(&blobstore, config, ctx, &req, data.map_err(Error::from))
        .timed()
        .await;
    log_perf(stats, &res, len);

    let metadata = res?;

    match args.delay {
        Some(delay) => {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        None => {}
    }

    eprintln!("Write committed: {:?}", metadata.content_id.blobstore_key());

    for _c in 0..args.read_count {
        read(&blobstore, ctx, &metadata).await?;
    }

    Ok(())
}

#[cfg(fbcode_build)]
const TEST_DATA_TTL: Option<Duration> = Some(Duration::from_secs(3600));

async fn open_blobstore(
    fb: FacebookInit,
    args: &BenchmarkArgs,
    blobstore_options: &BlobstoreOptions,
    readonly_storage: &ReadOnlyStorage,
    config_store: &ConfigStore,
) -> Result<Arc<dyn Blobstore>, Error> {
    let blobstore: Arc<dyn Blobstore> = match args.blobstore_type {
        BlobstoreType::Manifold => {
            #[cfg(fbcode_build)]
            {
                use manifoldblob::ManifoldBlob;
                use prefixblob::PrefixBlobstore;

                let bucket = args
                    .manifold_bucket
                    .as_ref()
                    .expect("Manifold bucket must be set when using manifold blobstore type");
                let put_behaviour = blobstore_options.put_behaviour;
                let manifold = ManifoldBlob::new(
                    fb,
                    bucket,
                    TEST_DATA_TTL,
                    blobstore_options.manifold_options.clone(),
                    put_behaviour,
                )?;
                let blobstore = PrefixBlobstore::new(manifold, format!("{}.", NAME));
                Arc::new(blobstore)
            }
            #[cfg(not(fbcode_build))]
            {
                unimplemented!("Accessing Manifold is not implemented in non fbcode builds");
            }
        }
        BlobstoreType::Memory => Arc::new(memblob::Memblob::default()),
        BlobstoreType::Xdb => {
            let tier_name = args
                .shardmap
                .as_ref()
                .expect("Shardmap must be set when using xdb blobstore type")
                .to_string();
            let blobstore = make_sql_blobstore_xdb(
                fb,
                tier_name,
                args.shard_count,
                blobstore_options,
                *readonly_storage,
                blobstore_options.put_behaviour,
                config_store,
            )
            .await?;
            Arc::new(blobstore)
        }
    };

    let blobstore: Arc<dyn Blobstore> = if args.memcache {
        Arc::new(new_memcache_blobstore(fb, blobstore, NAME, "")?)
    } else {
        blobstore
    };

    let blobstore: Arc<dyn Blobstore> = match args.cachelib_size {
        Some(size) => {
            #[cfg(fbcode_build)]
            {
                cachelib::init_cache(fb, cachelib::LruCacheConfig::new(size))?;

                let presence_pool = cachelib::get_or_create_pool(
                    "presence",
                    cachelib::get_available_space()? / 20,
                )?;
                let blob_pool =
                    cachelib::get_or_create_pool("blobs", cachelib::get_available_space()?)?;

                Arc::new(cacheblob::new_cachelib_blobstore_no_lease(
                    blobstore,
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
        None => blobstore,
    };

    // ThrottledBlob is a noop if no throttling requested
    let blobstore =
        Arc::new(ThrottledBlob::new(blobstore, blobstore_options.throttle_options).await);

    Ok(blobstore)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb).build::<BenchmarkArgs>()?;
    let args: BenchmarkArgs = app.args()?;

    let runtime = app.runtime();
    let logger = app.logger();
    let config_store = app.config_store();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let blobstore = runtime.block_on(open_blobstore(
        fb,
        &args,
        app.blobstore_options(),
        app.readonly_storage(),
        config_store,
    ))?;

    runtime.block_on(run_benchmark_filestore(&ctx, &args, blobstore))?;

    Ok(())
}
