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
use cas_client::CasClient;
use context::CoreContext;
pub use errors::ErrorKind;
use futures::stream;
use futures::StreamExt;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mononoke_types::ContentId;
use mononoke_types::MononokeDigest;
use stats::prelude::*;

#[cfg(fbcode_build)]
pub type MononokeCasClient<'a> = ScmCasClient<cas_client::RemoteExecutionCasdClient<'a>>;
#[cfg(not(fbcode_build))]
pub type MononokeCasClient<'a> = ScmCasClient<cas_client::DummyCasClient<'a>>;

define_stats! {
    prefix = "mononoke.cas_client";
    uploaded_manifests_count: timeseries(Rate, Sum),
    uploaded_files_count: timeseries(Rate, Sum),
}

const MAX_CONCURRENT_UPLOADS_TREES: usize = 200;
const MAX_CONCURRENT_UPLOADS_FILES: usize = 100;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UploadOutcome {
    Uploaded,
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
                let meta = filestore::get_metadata(blobstore, ctx, &fetch_key.into())
                    .await?
                    .ok_or(ErrorKind::ContentMetadataMissingInBlobstore(
                        filenode_id.clone().into_nodehash(),
                    ))?;
                let digest = MononokeDigest(meta.seeded_blake3, meta.total_size);
                if prior_lookup && self.lookup_digest(&digest).await? {
                    return Ok(UploadOutcome::AlreadyPresent);
                }
                digest
            }
        };
        let stream = filestore::fetch(blobstore, ctx.clone(), &fetch_key.into())
            .await?
            .ok_or(ErrorKind::MissingInBlobstore(
                filenode_id.clone().into_nodehash(),
            ))?;

        self.client.streaming_upload_blob(&digest, stream).await?;
        STATS::uploaded_files_count.add_value(1);
        Ok(UploadOutcome::Uploaded)
    }

    /// Upload a given file to a CAS backend (by Content Id)
    pub async fn upload_file_by_content_id<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        content_id: &ContentId,
        prior_lookup: bool,
    ) -> Result<UploadOutcome, Error> {
        let meta = filestore::get_metadata(blobstore, ctx, &content_id.to_owned().into())
            .await?
            .ok_or(ErrorKind::ContentMissingInBlobstore(content_id.clone()))?;
        let digest = MononokeDigest(meta.seeded_blake3, meta.total_size);
        if prior_lookup && self.lookup_digest(&digest).await? {
            return Ok(UploadOutcome::AlreadyPresent);
        }
        self.client
            .streaming_upload_blob(
                &digest,
                filestore::fetch(blobstore, ctx.clone(), &content_id.to_owned().into())
                    .await?
                    .ok_or(ErrorKind::ContentMissingInBlobstore(content_id.clone()))?,
            )
            .await?;
        STATS::uploaded_files_count.add_value(1);
        Ok(UploadOutcome::Uploaded)
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
        ids: Vec<(HgFileNodeId, Option<MononokeDigest>)>,
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
        ids: Vec<ContentId>,
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
        ids: Vec<(HgFileNodeId, Option<MononokeDigest>)>,
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
        self.client
            .streaming_upload_blob(&digest, bytes_stream)
            .await?;
        STATS::uploaded_manifests_count.add_value(1);
        Ok(UploadOutcome::Uploaded)
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
        manifest_ids: Vec<(HgAugmentedManifestId, Option<MononokeDigest>)>,
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
        manifest_ids: Vec<(HgAugmentedManifestId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Result<Vec<(HgAugmentedManifestId, UploadOutcome)>, Error> {
        self.upload_augmented_trees(ctx, blobstore, manifest_ids, prior_lookup)
            .await
            .into_iter()
            .collect()
    }
}
