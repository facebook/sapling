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
use mononoke_types::MononokeDigest;

const MAX_CONCURRENT_UPLOADS_TREES: usize = 200;
const MAX_CONCURRENT_UPLOADS_FILES: usize = 100;

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
    ) -> Result<(), Error> {
        if let Some(digest) = digest {
            if prior_lookup && self.lookup_digest(digest).await? {
                return Ok(());
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
                    return Ok(());
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
        Ok(())
    }

    /// Upload given file contents to a Cas backend (batched API)
    /// This implementation doesn't use batched API on CAS side yet, but it is planned to be implemented.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    pub async fn upload_file_contents<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        ids: Vec<(HgFileNodeId, Option<&MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Vec<Result<HgFileNodeId, Error>> {
        let uploads = ids.into_iter().map(move |id| async move {
            self.upload_file_content(ctx, blobstore, &id.0, id.1, prior_lookup)
                .await?;
            Ok(id.0)
        });
        stream::iter(uploads)
            .buffer_unordered(MAX_CONCURRENT_UPLOADS_FILES)
            .collect()
            .await
    }

    /// Upload given augmented tree to a CAS backend.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    pub async fn upload_augmented_tree<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_id: &HgAugmentedManifestId,
        prior_lookup: bool,
    ) -> Result<(), Error> {
        let augmented_manifest_envelope = manifest_id.load(ctx, blobstore).await?;
        let digest = MononokeDigest(
            augmented_manifest_envelope.augmented_manifest_id(),
            augmented_manifest_envelope.augmented_manifest_size(),
        );
        if prior_lookup && self.lookup_digest(&digest).await? {
            return Ok(());
        }
        let bytes_stream =
            augmented_manifest_envelope.into_content_addressed_manifest_blob(ctx, blobstore);
        self.client
            .streaming_upload_blob(&digest, bytes_stream)
            .await?;
        Ok(())
    }

    /// Upload given augmented trees to a Cas backend (batched API)
    /// This implementation doesn't use batched API on CAS side yet, but it is planned to be implemented.
    /// Prior lookup flag is used to check if a digest already exists in the CAS backend before fetching data from Mononoke blobstore.
    pub async fn upload_augmented_trees<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_ids: Vec<HgAugmentedManifestId>,
        prior_lookup: bool,
    ) -> Vec<Result<HgAugmentedManifestId, Error>> {
        let uploads = manifest_ids.into_iter().map(move |id| async move {
            self.upload_augmented_tree(ctx, blobstore, &id, prior_lookup)
                .await?;
            Ok(id)
        });
        stream::iter(uploads)
            .buffer_unordered(MAX_CONCURRENT_UPLOADS_TREES)
            .collect()
            .await
    }
}
