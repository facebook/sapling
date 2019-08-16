// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use cacheblob::{new_cachelib_blobstore_no_lease, new_memcache_blobstore_no_lease};
use cachelib;
use clap::{App, Arg, SubCommand};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure::Error;
use filestore::{self, FetchKey, FilestoreConfig, StoreRequest};
use futures::{Future, Stream};
use futures_stats::{FutureStats, Timed};
use manifoldblob::ThriftManifoldBlob;
use memblob;
use mononoke_types::ContentMetadata;
use prefixblob::PrefixBlobstore;
use sqlblob::Sqlblob;
use std::convert::TryInto;
use std::fmt::Debug;
use std::io::BufReader;
use std::sync::Arc;
use tokio::{codec, fs::File};

const NAME: &str = "benchmark_filestore";

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
        .map(|maybe_stream| maybe_stream.unwrap())
        .flatten_stream()
        .for_each(|_| Ok(()))
        .timed(move |stats, res| log_perf(stats, res, total_size))
        .map(move |_| content_metadata)
}

fn main() -> Result<(), Error> {
    let manifold_subcommand = SubCommand::with_name("manifold").arg(
        Arg::with_name("manifold-bucket")
            .takes_value(true)
            .required(false),
    );

    let memory_subcommand = SubCommand::with_name("memory");
    let xdb_subcommand = SubCommand::with_name("xdb")
        .arg(Arg::with_name("shardmap").takes_value(true).required(true))
        .arg(
            Arg::with_name("shard-count")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("myrouter-port")
                .long("myrouter-port")
                .takes_value(true)
                .required(false),
        );

    let app = App::new(NAME)
        .arg(
            Arg::with_name("input-capacity")
                .long("input-capacity")
                .takes_value(true)
                .required(false)
                .default_value("8192"),
        )
        .arg(
            Arg::with_name("chunk-size")
                .long("chunk-size")
                .takes_value(true)
                .required(false)
                .default_value("1048576"),
        )
        .arg(
            Arg::with_name("concurrency")
                .long("concurrency")
                .takes_value(true)
                .required(false)
                .default_value("1"),
        )
        .arg(Arg::with_name("memcache").long("memcache").required(false))
        .arg(
            Arg::with_name("cachelib-size")
                .long("cachelib-size")
                .takes_value(true)
                .required(false),
        )
        .arg(Arg::with_name("input").takes_value(true).required(true))
        .subcommand(manifold_subcommand)
        .subcommand(memory_subcommand)
        .subcommand(xdb_subcommand);

    let app = args::add_logger_args(app, true /* use glog */);
    let matches = app.get_matches();
    let input = matches.value_of("input").unwrap().to_string();

    let input_capacity: usize = matches
        .value_of("input-capacity")
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    let chunk_size: u64 = matches
        .value_of("chunk-size")
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    let concurrency: usize = matches
        .value_of("concurrency")
        .unwrap()
        .parse()
        .map_err(Error::from)?;

    let config = FilestoreConfig {
        chunk_size: Some(chunk_size),
        concurrency,
    };

    let mut runtime = tokio::runtime::Runtime::new().map_err(Error::from)?;

    let blob: Arc<dyn Blobstore> = match matches.subcommand() {
        ("manifold", Some(sub)) => {
            let bucket = sub.value_of("manifold-bucket").unwrap();
            let manifold = ThriftManifoldBlob::new(bucket).map_err(|e| -> Error { e.into() })?;
            let blobstore = PrefixBlobstore::new(manifold, format!("flat/{}.", NAME));
            Arc::new(blobstore)
        }
        ("memory", Some(_)) => Arc::new(memblob::LazyMemblob::new()),
        ("xdb", Some(sub)) => {
            let shardmap = sub.value_of("shardmap").unwrap().to_string();
            let shard_count = sub
                .value_of("shard-count")
                .unwrap()
                .parse()
                .map_err(Error::from)?;
            let fut = match sub.value_of("myrouter-port") {
                Some(port) => {
                    let port = port.parse().map_err(Error::from)?;
                    Sqlblob::with_myrouter(shardmap, port, shard_count)
                }
                None => Sqlblob::with_raw_xdb_shardmap(shardmap, shard_count),
            };
            let blobstore = runtime.block_on(fut)?;
            Arc::new(blobstore)
        }
        _ => unreachable!(),
    };

    let blob: Arc<dyn Blobstore> = if matches.is_present("memcache") {
        Arc::new(new_memcache_blobstore_no_lease(blob, NAME, "")?)
    } else {
        blob
    };

    let blob: Arc<dyn Blobstore> = match matches.value_of("cachelib-size") {
        Some(size) => {
            let cache_size_bytes = size.parse().map_err(Error::from)?;
            cachelib::init_cache_once(cachelib::LruCacheConfig::new(cache_size_bytes))?;

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

    let logger = args::get_logger(&matches);
    let ctx = CoreContext::new_with_logger(logger.clone());

    let fut = File::open(input)
        .and_then(|file| file.metadata())
        .from_err()
        .and_then({
            cloned!(blob, config, ctx);
            move |(file, metadata)| {
                let stdout = BufReader::with_capacity(input_capacity, file);
                let len: u64 = metadata.len().try_into().unwrap();
                eprintln!("Write start: {:?} B", len);

                let data = codec::FramedRead::new(stdout, codec::BytesCodec::new())
                    .map(|bytes_mut| bytes_mut.freeze())
                    .from_err();

                let req = StoreRequest::new(len);

                filestore::store(blob, &config, ctx, &req, data)
                    .timed(move |stats, res| log_perf(stats, res, len))
            }
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
