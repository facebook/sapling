// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use clap::ArgMatches;
use failure_ext::{Error, Result};
use futures::prelude::*;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use std::sync::Arc;

use blobstore::{Blobstore, CountedBlobstore};
use cacheblob::{new_memcache_blobstore, CacheBlobstoreExt};
use censoredblob::{CensoredBlob, SqlCensoredContentStore};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use futures::future;
use manifoldblob::ManifoldBlob;
use mercurial_types::{HgChangesetEnvelope, HgFileEnvelope, HgManifestEnvelope};
use metaconfig_types::{BlobConfig, Censoring};
use mononoke_types::{BlobstoreBytes, BlobstoreValue, FileContents, RepositoryId};
use prefixblob::PrefixBlobstore;
use slog::{info, warn, Logger};
use std::collections::HashMap;
use std::iter::FromIterator;

pub fn subcommand_blobstore_fetch(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let blobstore_args = args::parse_blobstore_args(&matches);
    let repo_id = try_boxfuture!(args::get_repo_id(&matches));

    let (_, config) = try_boxfuture!(args::get_config(&matches));
    let censoring = config.censoring;

    let (bucket, prefix) = match blobstore_args {
        BlobConfig::Manifold { bucket, prefix } => (bucket, prefix),
        bad => panic!("Unsupported blobstore: {:#?}", bad),
    };

    let ctx = CoreContext::test_mock();
    let key = sub_m.value_of("KEY").unwrap().to_string();
    let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
    let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
    let no_prefix = sub_m.is_present("no-prefix");

    let blobstore = ManifoldBlob::new_with_prefix(&bucket, &prefix);

    let maybe_censored_blobs_fut = match censoring {
        Censoring::Enabled => {
            let censored_blobs_store: Arc<_> = Arc::new(
                args::open_sql::<SqlCensoredContentStore>(&matches)
                    .expect("Failed to open the db with censored_blobs_store"),
            );

            censored_blobs_store
                .get_all_censored_blobs()
                .map_err(Error::from)
                .map(HashMap::from_iter)
                .map(Some)
                .left_future()
        }
        Censoring::Disabled => future::ok(None).right_future(),
    };

    let value_fut = maybe_censored_blobs_fut.and_then({
        cloned!(key, ctx);
        move |maybe_censored_blobs| {
            get_from_sources(
                use_memcache,
                blobstore,
                no_prefix,
                bucket,
                key.clone(),
                ctx,
                maybe_censored_blobs,
                repo_id,
            )
        }
    });

    value_fut
        .map({
            cloned!(key);
            move |value| {
                println!("{:?}", value);
                if let Some(value) = value {
                    let decode_as = decode_as.as_ref().and_then(|val| {
                        let val = val.as_str();
                        if val == "auto" {
                            detect_decode(&key, &logger)
                        } else {
                            Some(val)
                        }
                    });

                    match decode_as {
                        Some("changeset") => display(&HgChangesetEnvelope::from_blob(value.into())),
                        Some("manifest") => display(&HgManifestEnvelope::from_blob(value.into())),
                        Some("file") => display(&HgFileEnvelope::from_blob(value.into())),
                        // TODO: (rain1) T30974137 add a better way to print out file contents
                        Some("contents") => println!("{:?}", FileContents::from_blob(value.into())),
                        _ => (),
                    }
                }
            }
        })
        .boxify()
}

fn get_from_sources(
    use_memcache: Option<String>,
    blobstore: CountedBlobstore<ManifoldBlob>,
    no_prefix: bool,
    bucket: String,
    key: String,
    ctx: CoreContext,
    censored_blobs: Option<HashMap<String, String>>,
    repo_id: RepositoryId,
) -> BoxFuture<Option<BlobstoreBytes>, Error> {
    let empty_prefix = "".to_string();

    match use_memcache {
        Some(mode) => {
            let blobstore = new_memcache_blobstore(blobstore, "manifold", bucket).unwrap();
            let blobstore = match no_prefix {
                false => PrefixBlobstore::new(blobstore, repo_id.prefix()),
                true => PrefixBlobstore::new(blobstore, empty_prefix),
            };
            let blobstore = CensoredBlob::new(blobstore, censored_blobs);
            get_cache(ctx.clone(), &blobstore, key.clone(), mode)
        }
        None => {
            let blobstore = match no_prefix {
                false => PrefixBlobstore::new(blobstore, repo_id.prefix()),
                true => PrefixBlobstore::new(blobstore, empty_prefix),
            };
            let blobstore = CensoredBlob::new(blobstore, censored_blobs);
            blobstore.get(ctx, key.clone()).boxify()
        }
    }
}

fn display<T>(res: &Result<T>)
where
    T: fmt::Display + fmt::Debug,
{
    match res {
        Ok(val) => println!("---\n{}---", val),
        err => println!("{:?}", err),
    }
}

fn detect_decode(key: &str, logger: &Logger) -> Option<&'static str> {
    // Use a simple heuristic to figure out how to decode this key.
    if key.find("hgchangeset.").is_some() {
        info!(logger, "Detected changeset key");
        Some("changeset")
    } else if key.find("hgmanifest.").is_some() {
        info!(logger, "Detected manifest key");
        Some("manifest")
    } else if key.find("hgfilenode.").is_some() {
        info!(logger, "Detected file key");
        Some("file")
    } else if key.find("content.").is_some() {
        info!(logger, "Detected content key");
        Some("contents")
    } else {
        warn!(
            logger,
            "Unable to detect how to decode this blob based on key";
            "key" => key,
        );
        None
    }
}

fn get_cache<B: CacheBlobstoreExt>(
    ctx: CoreContext,
    blobstore: &B,
    key: String,
    mode: String,
) -> BoxFuture<Option<BlobstoreBytes>, Error> {
    if mode == "cache-only" {
        blobstore.get_cache_only(key)
    } else if mode == "no-fill" {
        blobstore.get_no_cache_fill(ctx, key)
    } else {
        blobstore.get(ctx, key)
    }
}
