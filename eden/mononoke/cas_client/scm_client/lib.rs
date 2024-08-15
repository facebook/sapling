/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use bytes::BytesMut;
use cas_client::CasClient;
use context::CoreContext;
pub use errors::ErrorKind;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mononoke_types::ContentId;
use mononoke_types::MononokeDigest;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.cas_client";
    uploaded_manifests_count: timeseries(Rate, Sum),
    uploaded_files_count: timeseries(Rate, Sum),
    uploaded_bytes: dynamic_histogram("{}.uploaded_bytes", (repo_name: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    small_blobs_uploaded_bytes: dynamic_histogram("{}.small_blobs.uploaded_bytes", (repo_name: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    large_blobs_uploaded_bytes: dynamic_histogram("{}.large_blobs.uploaded_bytes", (repo_name: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

const MAX_CONCURRENT_UPLOADS_TREES: usize = 200;
const MAX_CONCURRENT_UPLOADS_FILES: usize = 100;
const MAX_BYTES_FOR_INLINE_UPLOAD: u64 = 3_000_000;
const SMALL_BLOBS_THRESHOLD: u64 = 2_621_440;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UploadOutcome {
    Uploaded(u64),
    AlreadyPresent,
}

pub struct ScmCasClient<Client>
where
    Client: CasClient,
{
    pub client: Client,
}

impl<Client> ScmCasClient<Client>
where
    Client: CasClient,
{
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    async fn lookup_digest(&self, digest: &MononokeDigest) -> Result<bool, Error> {
        self.client.lookup_blob(digest).await
    }

    fn log_upload_size(&self, size: u64) {
        let repo_name = self.client.repo_name().to_string();
        STATS::uploaded_bytes.add_value(size as i64, (repo_name.clone(),));
        if size <= SMALL_BLOBS_THRESHOLD {
            STATS::small_blobs_uploaded_bytes.add_value(size as i64, (repo_name,));
        } else {
            STATS::large_blobs_uploaded_bytes.add_value(size as i64, (repo_name,));
        }
    }

    async fn get_file_digest<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        content_id: &ContentId,
    ) -> Result<MononokeDigest, Error> {
        let meta = filestore::get_metadata(blobstore, ctx, &content_id.to_owned().into())
            .await?
            .ok_or(ErrorKind::ContentMissingInBlobstore(content_id.clone()))?;
        Ok(MononokeDigest(meta.seeded_blake3, meta.total_size))
    }

    async fn fetch_upload_file<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        content_id: ContentId,  // for fetching
        digest: MononokeDigest, // for uploading
    ) -> Result<UploadOutcome, Error> {
        let stream = filestore::fetch(blobstore, ctx.clone(), &content_id.into())
            .await?
            .ok_or(ErrorKind::MissingInBlobstore(content_id))?;
        if digest.1 <= MAX_BYTES_FOR_INLINE_UPLOAD {
            let bytes_to_upload = stream.try_collect::<BytesMut>().await?;
            self.client
                .upload_blob(&digest, bytes_to_upload.into())
                .await?;
        } else {
            self.client.streaming_upload_blob(&digest, stream).await?;
        }
        STATS::uploaded_files_count.add_value(1);
        self.log_upload_size(digest.1);
        Ok(UploadOutcome::Uploaded(digest.1))
    }

    /// Upload a given file to a CAS backend.
    /// Digest of a file can be known a priori (from Augmented Manifest, for example), but it is not required.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    pub async fn upload_file_content<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        filenode_id: &HgFileNodeId,
        digest: Option<&MononokeDigest>,
        prior_lookup: bool,
    ) -> Result<UploadOutcome, Error> {
        if let Some(digest) = digest {
            if prior_lookup && self.lookup_digest(digest).await? {
                return Ok(UploadOutcome::AlreadyPresent);
            }
        }
        let fetch_key = filenode_id.load(ctx, blobstore).await?.content_id();
        let digest = match digest {
            Some(digest) => digest.clone(),
            None => {
                let digest = self.get_file_digest(ctx, blobstore, &fetch_key).await?;
                if prior_lookup && self.lookup_digest(&digest).await? {
                    return Ok(UploadOutcome::AlreadyPresent);
                }
                digest
            }
        };
        self.fetch_upload_file(ctx, blobstore, fetch_key, digest)
            .await
    }

    /// Upload a given file to a CAS backend (by Content Id)
    pub async fn upload_file_by_content_id<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        content_id: &ContentId,
        prior_lookup: bool,
    ) -> Result<UploadOutcome, Error> {
        let digest = self.get_file_digest(ctx, blobstore, content_id).await?;
        if prior_lookup && self.lookup_digest(&digest).await? {
            return Ok(UploadOutcome::AlreadyPresent);
        }
        self.fetch_upload_file(ctx, blobstore, content_id.clone(), digest)
            .await
    }

    /// Upload given file contents to a Cas backend (batched API)
    /// This implementation doesn't use batched API on CAS side yet, but it is planned to be implemented.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    /// Digests of files can be known a priori (from Augmented Manifest, for example), but it is not required.
    /// Caller can analyse individual results.
    /// Do not increment uploaded_files_count counter, since the method is calling into the individual upload method.
    pub async fn upload_file_contents<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        ids: impl IntoIterator<Item = (HgFileNodeId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Vec<Result<(HgFileNodeId, UploadOutcome), Error>> {
        let uploads = ids.into_iter().map(move |id| async move {
            let outcome = self
                .upload_file_content(ctx, blobstore, &id.0, id.1.as_ref(), prior_lookup)
                .await?;
            Ok((id.0, outcome))
        });
        stream::iter(uploads)
            .buffer_unordered(MAX_CONCURRENT_UPLOADS_FILES)
            .collect()
            .await
    }

    /// Upload given file contents to a Cas backend (batched API) (by Content Id)
    /// Do not increment uploaded_files_count counter, since the method is calling into the individual upload method.
    pub async fn upload_files_by_content_id<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        ids: impl IntoIterator<Item = ContentId>,
        prior_lookup: bool,
    ) -> Vec<Result<(ContentId, UploadOutcome), Error>> {
        stream::iter(ids.into_iter().map(move |id| async move {
            let outcome = self
                .upload_file_by_content_id(ctx, blobstore, &id, prior_lookup)
                .await?;
            Ok((id, outcome))
        }))
        .buffer_unordered(MAX_CONCURRENT_UPLOADS_FILES)
        .collect()
        .await
    }

    pub async fn ensure_upload_file_contents<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        ids: impl IntoIterator<Item = (HgFileNodeId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Result<Vec<(HgFileNodeId, UploadOutcome)>, Error> {
        self.upload_file_contents(ctx, blobstore, ids, prior_lookup)
            .await
            .into_iter()
            .collect()
    }

    pub async fn ensure_upload_files_by_content_id<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        ids: Vec<ContentId>,
        prior_lookup: bool,
    ) -> Result<Vec<(ContentId, UploadOutcome)>, Error> {
        self.upload_files_by_content_id(ctx, blobstore, ids, prior_lookup)
            .await
            .into_iter()
            .collect()
    }

    /// Upload given augmented tree to a CAS backend.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    /// Digest of a tree can be known a priori (from Augmented Manifest, for example), but it is not required.
    pub async fn upload_augmented_tree<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_id: &HgAugmentedManifestId,
        digest: Option<&MononokeDigest>,
        prior_lookup: bool,
    ) -> Result<UploadOutcome, Error> {
        let mut digest_checked = false;
        if let Some(digest) = digest {
            if prior_lookup && self.lookup_digest(digest).await? {
                return Ok(UploadOutcome::AlreadyPresent);
            }
            digest_checked = true;
        }
        let augmented_manifest_envelope = manifest_id.load(ctx, blobstore).await?;
        let digest = MononokeDigest(
            augmented_manifest_envelope.augmented_manifest_id(),
            augmented_manifest_envelope.augmented_manifest_size(),
        );
        if !digest_checked && prior_lookup && self.lookup_digest(&digest).await? {
            return Ok(UploadOutcome::AlreadyPresent);
        }
        let bytes_stream =
            augmented_manifest_envelope.into_content_addressed_manifest_blob(ctx, blobstore);
        if digest.1 <= MAX_BYTES_FOR_INLINE_UPLOAD {
            let bytes_to_upload = bytes_stream.try_collect::<BytesMut>().await?;
            self.client
                .upload_blob(&digest, bytes_to_upload.into())
                .await?;
        } else {
            self.client
                .streaming_upload_blob(&digest, bytes_stream)
                .await?;
        }
        STATS::uploaded_manifests_count.add_value(1);
        self.log_upload_size(digest.1);
        Ok(UploadOutcome::Uploaded(digest.1))
    }

    /// Upload given augmented trees to a Cas backend (batched API)
    /// This implementation doesn't use batched API on CAS side yet, but it is planned to be implemented.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    /// Digests of trees can be known a priori (from Augmented Manifest, for example), but it is not required.
    /// Caller can analyse individual results.
    /// Do not increment uploaded_manifests_count counter, since the method is calling into the individual upload method.
    pub async fn upload_augmented_trees<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_ids: impl IntoIterator<Item = (HgAugmentedManifestId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Vec<Result<(HgAugmentedManifestId, UploadOutcome), Error>> {
        let uploads = manifest_ids.into_iter().map(move |id| async move {
            let outcome = self
                .upload_augmented_tree(ctx, blobstore, &id.0, id.1.as_ref(), prior_lookup)
                .await?;
            Ok((id.0, outcome))
        });
        stream::iter(uploads)
            .buffer_unordered(MAX_CONCURRENT_UPLOADS_TREES)
            .collect()
            .await
    }

    pub async fn ensure_upload_augmented_trees<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_ids: impl IntoIterator<Item = (HgAugmentedManifestId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Result<Vec<(HgAugmentedManifestId, UploadOutcome)>, Error> {
        self.upload_augmented_trees(ctx, blobstore, manifest_ids, prior_lookup)
            .await
            .into_iter()
            .collect()
    }

    pub async fn is_augmented_tree_uploaded<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_id: &HgAugmentedManifestId,
    ) -> Result<bool, Error> {
        let augmented_manifest_envelope = manifest_id.load(ctx, blobstore).await?;
        let digest = MononokeDigest(
            augmented_manifest_envelope.augmented_manifest_id(),
            augmented_manifest_envelope.augmented_manifest_size(),
        );
        self.lookup_digest(&digest).await
    }
}
