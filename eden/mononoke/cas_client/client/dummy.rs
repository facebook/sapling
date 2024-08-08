/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::BlobstoreIsPresent;
use blobstore::PutBehaviour;
use bytes::Bytes;
use bytes::BytesMut;
use context::CoreContext;
use fileblob::Fileblob;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_types::BlobstoreBytes;
use mononoke_types::MononokeDigest;

use crate::CasClient;

const CAS_STORE_ENV: &str = "CAS_STORE_PATH";
const CAS_STORE_DIR: &str = "cas_store";

/// A CasClient that store blobs in a local file blobstore with predefined path.
/// <CAS_STORE_PATH>/<repo_name>/cas_store or
/// <provided_path>/<repo_name>/cas_store
/// This is useful for testing, and also allow to isolate cas storage per test.
pub struct DummyCasClient<'a> {
    ctx: &'a CoreContext,
    repo: &'a str,
    file_blobstore: Fileblob,
}

impl<'a> DummyCasClient<'a> {
    pub fn new(ctx: &'a CoreContext, repo: &'a str) -> Result<Self, Error> {
        let storage_cas = std::env::var(CAS_STORE_ENV).map(PathBuf::from)?;
        let blobstore_path = storage_cas.join(repo).join(CAS_STORE_DIR);
        let put_behaviour = PutBehaviour::IfAbsent;
        let file_blobstore = Fileblob::create(blobstore_path, put_behaviour)?;
        Ok(Self {
            ctx,
            repo,
            file_blobstore,
        })
    }

    pub fn new_with_storage_path<P>(
        ctx: &'a CoreContext,
        repo: &'a str,
        path: P,
    ) -> Result<Self, Error>
    where
        P: Into<PathBuf>,
    {
        let put_behaviour = PutBehaviour::IfAbsent;
        let file_blobstore =
            Fileblob::create(path.into().join(repo).join(CAS_STORE_DIR), put_behaviour)?;
        Ok(Self {
            ctx,
            repo,
            file_blobstore,
        })
    }
}

/// A CasClient that does nothing. All operations are using an on-disk blobstore.
#[async_trait::async_trait]
impl<'a> CasClient for DummyCasClient<'a> {
    async fn streaming_upload_blob(
        &self,
        digest: &MononokeDigest,
        bytes_stream: impl Stream<Item = Result<Bytes, Error>> + Send,
    ) -> Result<(), Error> {
        let key = digest.to_string();
        let bytes_to_upload =
            BlobstoreBytes::from_bytes(bytes_stream.try_collect::<BytesMut>().await?);
        self.file_blobstore
            .put(self.ctx, key, bytes_to_upload)
            .await
    }

    async fn upload_blob(&self, digest: &MononokeDigest, bytes: Bytes) -> Result<(), Error> {
        let key = digest.to_string();
        self.file_blobstore
            .put(self.ctx, key, BlobstoreBytes::from_bytes(bytes))
            .await
    }

    async fn upload_blobs(&self, blobs: Vec<(MononokeDigest, Bytes)>) -> Result<(), Error> {
        stream::iter(
            blobs
                .into_iter()
                .map(move |(digest, blob)| async move { self.upload_blob(&digest, blob).await }),
        )
        .buffer_unordered(100)
        .try_collect()
        .await
    }

    async fn lookup_blob(&self, digest: &MononokeDigest) -> Result<bool, Error> {
        let key = digest.to_string();
        match self.file_blobstore.is_present(self.ctx, &key).await? {
            BlobstoreIsPresent::Present => Ok(true),
            BlobstoreIsPresent::Absent => Ok(false),
            BlobstoreIsPresent::ProbablyNotPresent(error) => Err(error),
        }
    }

    async fn missing_digests<'b>(
        &self,
        digests: &'b [MononokeDigest],
    ) -> Result<Vec<MononokeDigest>, Error> {
        let mut missing = vec![];
        for digest in digests {
            if !self.lookup_blob(digest).await? {
                missing.push(digest.clone());
            }
        }
        Ok(missing)
    }

    fn repo_name(&self) -> &str {
        self.repo
    }
}
