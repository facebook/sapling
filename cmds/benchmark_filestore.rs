/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#[deny(warnings)]
use blobstore::Blobstore;
use bytes::Bytes;
use cacheblob::{new_cachelib_blobstore_no_lease, new_memcache_blobstore_no_lease};
use clap::{App, Arg, SubCommand};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure::Error;
use failure_ext::format_err;
use fbinit::FacebookInit;
use filestore::{self, FetchKey, FilestoreConfig, StoreRequest};
use futures::{stream::iter_ok, Future, IntoFuture, Stream};
use futures_ext::{FutureExt, StreamExt};
use futures_stats::{FutureStats, Timed};
use manifoldblob::ThriftManifoldBlob;
use mononoke_types::{ContentMetadata, MononokeId};
use prefixblob::PrefixBlobstore;
use rand::Rng;
use sqlblob::Sqlblob;
use std::fmt::Debug;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;
use tokio::{codec, fs::File};

const NAME: &str = "benchmark_filestore";

const CMD_MANIFOLD: &str = "manifold";
const CMD_MEMORY: &str = "memory";
const CMD_XDB: &str = "xdb";

const ARG_MANIFOLD_BUCKET: &str = "manifold-bucket";
const ARG_SHARDMAP: &str = "shardmap";
const ARG_SHARD_COUNT: &str = "shard-count";
const ARG_MYROUTER_PORT: &str = "myrouter-port";
const ARG_INPUT_CAPACITY: &str = "input-capacity";
const ARG_CHUNK_SIZE: &str = "chunk-size";
const ARG_CONCURRENCY: &str = "concurrency";
const ARG_MEMCACHE: &str = "memcache";
const ARG_CACHELIB_SIZE: &str = "cachelib-size";
const ARG_INPUT: &str = "input";
const ARG_DELAY: &str = "delay";
const ARG_DEBUG: &str = "debug";
const ARG_RANDOMIZE: &str = "randomize";

fn log_perf<I, E: Debug>(stats: FutureStats, res: Result<&I, &E>, len: u64) -> Result<(), ()> {
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

    Ok(())
}

fn read<B: Blobstore + Clone>(
    blob: B,
    ctx: CoreContext,
    content_metadata: ContentMetadata,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    let ContentMetadata {
        content_id,
        total_size,
        ..
    } = content_metadata.clone();

    let key = FetchKey::Canonical(content_id);
    eprintln!("Fetch start: {:?} ({:?} B)", key, total_size);

    filestore::fetch(&blob, ctx, &key)
        .and_then(|maybe_stream| match maybe_stream {
            Some(stream) => Ok(stream),
            None => Err(format_err!("Fetch failed: no stream")),
        })
        .flatten_stream()
        .for_each(|_| Ok(()))
        .timed(move |stats, res| log_perf(stats, res, total_size))
        .map(move |_| content_metadata)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let manifold_subcommand = SubCommand::with_name("manifold").arg(
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
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_MYROUTER_PORT)
                .long("myrouter-port")
                .takes_value(true)
                .required(false),
        );

    let app = App::new(NAME)
        .arg(
            Arg::with_name(ARG_INPUT_CAPACITY)
                .long("input-capacity")
                .takes_value(true)
                .required(false)
                .default_value("8192"),
        )
        .arg(
            Arg::with_name(ARG_CHUNK_SIZE)
                .long("chunk-size")
                .takes_value(true)
                .required(false)
                .default_value("1048576"),
        )
        .arg(
            Arg::with_name(ARG_CONCURRENCY)
                .long("concurrency")
                .takes_value(true)
                .required(false)
                .default_value("1"),
        )
        .arg(
            Arg::with_name(ARG_MEMCACHE)
                .long("memcache")
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_CACHELIB_SIZE)
                .long("cachelib-size")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_DELAY)
                .long("delay-after-write")
                .takes_value(true)
                .required(false),
        )
        .arg(
            // This is read by args::init_logging
            Arg::with_name(ARG_DEBUG).long("debug").required(false),
        )
        .arg(
            Arg::with_name(ARG_RANDOMIZE)
                .long("randomize")
                .required(false),
        )
        .arg(Arg::with_name(ARG_INPUT).takes_value(true).required(true))
        .subcommand(manifold_subcommand)
        .subcommand(memory_subcommand)
        .subcommand(xdb_subcommand);

    let app = args::add_logger_args(app);
    let matches = app.get_matches();
    let input = matches.value_of("input").unwrap().to_string();

    let input_capacity: usize = matches
        .value_of(ARG_INPUT_CAPACITY)
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    let chunk_size: u64 = matches
        .value_of(ARG_CHUNK_SIZE)
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    let concurrency: usize = matches
        .value_of(ARG_CONCURRENCY)
        .unwrap()
        .parse()
        .map_err(Error::from)?;

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

    let mut runtime = tokio::runtime::Runtime::new().map_err(Error::from)?;

    let blob: Arc<dyn Blobstore> = match matches.subcommand() {
        (CMD_MANIFOLD, Some(sub)) => {
            let bucket = sub.value_of(ARG_MANIFOLD_BUCKET).unwrap();
            let manifold =
                ThriftManifoldBlob::new(fb, bucket).map_err(|e| -> Error { e.into() })?;
            let blobstore = PrefixBlobstore::new(manifold, format!("flat/{}.", NAME));
            Arc::new(blobstore)
        }
        (CMD_MEMORY, Some(_)) => Arc::new(memblob::LazyMemblob::new()),
        (CMD_XDB, Some(sub)) => {
            let shardmap = sub.value_of(ARG_SHARDMAP).unwrap().to_string();
            let shard_count = sub
                .value_of(ARG_SHARD_COUNT)
                .unwrap()
                .parse()
                .map_err(Error::from)?;
            let fut = match sub.value_of(ARG_MYROUTER_PORT) {
                Some(port) => {
                    let port = port.parse().map_err(Error::from)?;
                    Sqlblob::with_myrouter(fb, shardmap, port, shard_count)
                }
                None => Sqlblob::with_raw_xdb_shardmap(fb, shardmap, shard_count),
            };
            let blobstore = runtime.block_on(fut)?;
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
            let cache_size_bytes = size.parse().map_err(Error::from)?;
            cachelib::init_cache_once(fb, cachelib::LruCacheConfig::new(cache_size_bytes))?;

            let presence_pool =
                cachelib::get_or_create_pool("presence", cachelib::get_available_space()? / 20)?;
            let blob_pool =
                cachelib::get_or_create_pool("blobs", cachelib::get_available_space()?)?;

            Arc::new(new_cachelib_blobstore_no_lease(
                blob,
                Arc::new(blob_pool),
                Arc::new(presence_pool),
            ))
        }
        None => blob,
    };

    eprintln!("Test with {:?}, writing into {:?}", config, blob);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let fut = File::open(input)
        .and_then(|file| file.metadata())
        .from_err()
        .and_then({
            cloned!(blob, config, ctx);
            move |(file, metadata)| {
                let stdout = BufReader::with_capacity(input_capacity, file);
                let len = metadata.len();

                let data = codec::FramedRead::new(stdout, codec::BytesCodec::new())
                    .map(|bytes_mut| bytes_mut.freeze())
                    .from_err();

                let (len, data) = if randomize {
                    let bytes = rand::thread_rng().gen::<[u8; 32]>();
                    let bytes = Bytes::from(&bytes[..]);
                    (
                        len + (bytes.len() as u64),
                        iter_ok(vec![bytes]).chain(data).left_stream(),
                    )
                } else {
                    (len, data.right_stream())
                };

                eprintln!("Write start: {:?} B", len);

                let req = StoreRequest::new(len);

                filestore::store(blob, &config, ctx, &req, data)
                    .timed(move |stats, res| log_perf(stats, res, len))
            }
        })
        .and_then(move |res| match delay {
            Some(delay) => tokio_timer::sleep(delay)
                .from_err()
                .map(move |_| res)
                .left_future(),
            None => {
                let res: Result<_, Error> = Ok(res);
                res.into_future().right_future()
            }
        })
        .inspect(|meta| {
            eprintln!("Write committed: {:?}", meta.content_id.blobstore_key());
        })
        .and_then({
            cloned!(blob, ctx);
            move |res| read(blob, ctx, res)
        })
        .and_then({
            cloned!(blob, ctx);
            move |res| read(blob, ctx, res)
        });

    runtime.block_on(fut)?;

    Ok(())
}
