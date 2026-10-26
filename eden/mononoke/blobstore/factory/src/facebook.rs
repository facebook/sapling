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

    /// Override the bucket in Hedwig's client identity, sharing its cache and
    /// download limits across Manifold buckets. Reuses --manifold-api-key;
    /// storage reads and writes still use their configured buckets.
    #[clap(
        long,
        requires = "manifold_api_key",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub manifold_hedwig_cache_bucket: Option<String>,

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

    /// The maximum number of automatic retries during write
    /// operation.
    #[clap(long)]
    pub manifold_write_retries: Option<i16>,
}

impl From<ManifoldArgs> for ManifoldOptions {
    fn from(args: ManifoldArgs) -> ManifoldOptions {
        ManifoldOptions {
            api_key: args.manifold_api_key,
            hedwig_cache_bucket: args.manifold_hedwig_cache_bucket,
            weak_consistency_ms: args.manifold_weak_consistency_ms,
            parallel_downloads: args.manifold_parallel_downloads,
            read_timeout_ms: args.manifold_read_timeout_ms,
            read_retries: args.manifold_read_retries,
            write_retries: args.manifold_write_retries,
        }
    }
}

pub fn make_manifold_blobstore_enumerable_with_unlink(
    fb: FacebookInit,
    prefix: &str,
    bucket: &str,
    ttl: Option<Duration>,
    manifold_options: &ManifoldOptions,
    put_behaviour: PutBehaviour,
) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>, Error> {
    let manifold = ManifoldBlob::new(fb, bucket, ttl, manifold_options.clone(), put_behaviour)
        .context("Error while opening state for blob store")?;

    Ok(Arc::new(PrefixBlobstore::new(manifold, prefix.to_string()))
        as Arc<dyn BlobstoreEnumerableWithUnlink>)
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use mononoke_macros::mononoke;

    use super::*;

    #[derive(Parser)]
    struct TestArgs {
        #[clap(flatten)]
        manifold: ManifoldArgs,
    }

    #[mononoke::test]
    fn shared_hedwig_cache_preserves_storage_credentials() {
        for bucket in ["shared-cache", "another-cache"] {
            let args = TestArgs::try_parse_from([
                "test",
                "--manifold-api-key=storage-key",
                "--manifold-hedwig-cache-bucket",
                bucket,
            ])
            .unwrap();
            let options = ManifoldOptions::from(args.manifold);
            assert_eq!(options.api_key.as_deref(), Some("storage-key"));
            assert_eq!(options.hedwig_cache_bucket.as_deref(), Some(bucket));
        }
    }

    #[mononoke::test]
    fn shared_hedwig_cache_is_optional() {
        let defaults = TestArgs::try_parse_from(["test"]).unwrap();
        assert!(
            ManifoldOptions::from(defaults.manifold)
                .hedwig_cache_bucket
                .is_none()
        );
    }

    #[mononoke::test]
    fn shared_hedwig_cache_requires_bucket_and_api_key() {
        assert!(
            TestArgs::try_parse_from(["test", "--manifold-hedwig-cache-bucket=shared-cache",])
                .is_err()
        );
        assert!(
            TestArgs::try_parse_from([
                "test",
                "--manifold-api-key=storage-key",
                "--manifold-hedwig-cache-bucket=",
            ])
            .is_err()
        );
    }
}
