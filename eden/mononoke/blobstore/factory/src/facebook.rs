/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use blobstore::BlobstoreEnumerableWithUnlink;
use blobstore::ErrorKind;
use blobstore::PutBehaviour;
use clap::Args;
use fbinit::FacebookInit;
use manifoldblob::ManifoldBlob;
use manifoldblob::ManifoldOptions;
use prefixblob::PrefixBlobstore;

/// Command line arguments for controlling Manifold.
#[derive(Clone, Debug, Args)]
pub struct ManifoldArgs {
    /// Manifold API key
    #[clap(long)]
    pub manifold_api_key: Option<String>,

    /// Manifold Weak Consistency max age millis
    #[clap(long)]
    pub manifold_weak_consistency_ms: Option<i64>,

    /// Parallelize download of blob chunks during read ops
    /// with the given max degree of parallelism.
    #[clap(long)]
    pub manifold_parallel_downloads: Option<u32>,

    /// Maximum timeout for read operations.
    /// NOTE: This includes time taken for original request
    /// AND time taken for retries.
    #[clap(long)]
    pub manifold_read_timeout_ms: Option<u32>,

    /// The maximum number of automatic retries during read
    /// operation.
    #[clap(long)]
    pub manifold_read_retries: Option<i16>,
}

impl From<ManifoldArgs> for ManifoldOptions {
    fn from(args: ManifoldArgs) -> ManifoldOptions {
        ManifoldOptions {
            api_key: args.manifold_api_key,
            weak_consistency_ms: args.manifold_weak_consistency_ms,
            parallel_downloads: args.manifold_parallel_downloads,
            read_timeout_ms: args.manifold_read_timeout_ms,
            read_retries: args.manifold_read_retries,
        }
    }
}

pub fn make_manifold_blobstore(
    fb: FacebookInit,
    prefix: &str,
    bucket: &str,
    ttl: Option<Duration>,
    manifold_options: &ManifoldOptions,
    put_behaviour: PutBehaviour,
) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>, Error> {
    let manifold = ManifoldBlob::new(fb, bucket, ttl, manifold_options.clone(), put_behaviour)
        .context(ErrorKind::StateOpen)?;

    Ok(Arc::new(PrefixBlobstore::new(manifold, prefix.to_string()))
        as Arc<dyn BlobstoreEnumerableWithUnlink>)
}
