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

use blobstore::Blobstore;
use cacheblob::{new_memcache_blobstore, CacheBlobstoreExt};
use cmdlib::args;
use context::CoreContext;
use manifoldblob::ManifoldBlob;
use mercurial_types::{HgChangesetEnvelope, HgFileEnvelope, HgManifestEnvelope};
use metaconfig_types::BlobConfig;
use mononoke_types::{BlobstoreBytes, BlobstoreValue, FileContents};
use prefixblob::PrefixBlobstore;
use slog::{info, warn, Logger};

pub fn subcommand_blobstore_fetch(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let blobstore_args = args::parse_blobstore_args(&matches);
    let repo_id = try_boxfuture!(args::get_repo_id(&matches));

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

    match (use_memcache, no_prefix) {
        (None, false) => {
            let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
            blobstore.get(ctx, key.clone()).boxify()
        }
        (None, true) => blobstore.get(ctx, key.clone()).boxify(),
        (Some(mode), false) => {
            let blobstore = new_memcache_blobstore(blobstore, "manifold", bucket).unwrap();
            let blobstore = PrefixBlobstore::new(blobstore, repo_id.prefix());
            get_cache(ctx.clone(), &blobstore, key.clone(), mode)
        }
        (Some(mode), true) => {
            let blobstore = new_memcache_blobstore(blobstore, "manifold", bucket).unwrap();
            get_cache(ctx.clone(), &blobstore, key.clone(), mode)
        }
    }
    .map(move |value| {
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
    })
    .boxify()
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
