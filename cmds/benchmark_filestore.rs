// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use clap::{App, Arg};
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use filestore::{self, FetchKey, FilestoreConfig, StoreRequest};
use futures::{Future, Stream};
use futures_ext::FutureExt;
use futures_stats::Timed;
use manifoldblob::ThriftManifoldBlob;
use memblob;
use prefixblob::PrefixBlobstore;
use std::convert::TryInto;
use std::io::BufReader;
use std::sync::Arc;
use tokio::{codec, fs::File};

fn main() -> Result<(), Error> {
    let app = App::new("benchmark filestore")
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
        .arg(
            Arg::with_name("manifold-bucket")
                .long("manifold-bucket")
                .takes_value(true)
                .required(false),
        )
        .arg(Arg::with_name("input").takes_value(true).required(true));

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

    let blob: Arc<dyn Blobstore> = match matches.value_of("manifold-bucket") {
        Some(bucket) => {
            let manifold = ThriftManifoldBlob::new(bucket).map_err(|e| -> Error { e.into() })?;
            let blobstore = PrefixBlobstore::new(manifold, "flat/benchmark_filstore.");
            Arc::new(blobstore)
        }
        None => Arc::new(memblob::LazyMemblob::new()),
    };

    eprintln!("Test with {:?}, writing into {:?}", config, blob);

    let ctx = CoreContext::test_mock();

    let fut = File::open(input)
        .and_then(|file| file.metadata())
        .from_err()
        .and_then({
            cloned!(blob, config, ctx);
            move |(file, metadata)| {
                let stdout = BufReader::with_capacity(input_capacity, file);
                let len: u64 = metadata.len().try_into().unwrap();
                eprintln!("Starting... File size is: {:?} B", len);

                let data = codec::FramedRead::new(stdout, codec::BytesCodec::new())
                    .map(|bytes_mut| bytes_mut.freeze())
                    .from_err();

                let req = StoreRequest::new(len);

                filestore::store(&blob, &config, ctx, &req, data).timed(move |stats, x| {
                    let bytes_per_ns = (len as f64) / (stats.completion_time.as_nanos() as f64);
                    let mbytes_per_s =
                        bytes_per_ns * (10_u128.pow(9) as f64) / (2_u128.pow(20) as f64);
                    let gb_per_s = mbytes_per_s * 8_f64 / 1024_f64;
                    eprintln!(
                        "Done writing: {:.2} MB/s ({:.2} Gb/s) ({:?})",
                        mbytes_per_s, gb_per_s, stats
                    );
                    eprintln!("hello {:?}", x);
                    Ok(())
                })
            }
        })
        .and_then({
            cloned!(blob, ctx);
            move |res| {
                let key = FetchKey::Canonical(res.content_id);
                eprintln!("Fetch start? {:?}", key);

                filestore::fetch(&blob, ctx, &key)
                    .map(|maybe_stream| maybe_stream.unwrap())
                    .flatten_stream()
                    .for_each(|_| Ok(()))
                    .timed(|stats, _| {
                        eprintln!("Done reading: {:?}", stats);
                        Ok(())
                    })
            }
        });

    tokio::run(
        fut.map_err(|e| {
            eprintln!("Error: {:?}", e);
            ()
        })
        .discard(),
    );

    Ok(())
}
