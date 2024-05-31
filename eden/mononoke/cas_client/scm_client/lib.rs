/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;
use std::cmp;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use cas_client::CasClient;
use context::CoreContext;
pub use errors::ErrorKind;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mononoke_types::MononokeDigest;
use slog::info;

#[cfg(fbcode_build)]
pub type MononokeCasClient<'a> = ScmCasClient<cas_client::RemoteExecutionCasdClient<'a>>;
#[cfg(not(fbcode_build))]
pub type MononokeCasClient<'a> = ScmCasClient<cas_client::DummyCasClient<'a>>;

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
    /// Digests of files can be known a priori (from Augmented Manifest, for example), but it is not required.
    /// Caller can analyse individual results.
    pub async fn upload_file_contents<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        ids: Vec<(HgFileNodeId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Vec<Result<HgFileNodeId, Error>> {
        let uploads = ids.into_iter().map(move |id| async move {
            self.upload_file_content(ctx, blobstore, &id.0, id.1.as_ref(), prior_lookup)
                .await?;
            Ok(id.0)
        });
        stream::iter(uploads)
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
    ) -> Result<Vec<HgFileNodeId>, Error> {
        self.upload_file_contents(ctx, blobstore, ids, prior_lookup)
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
    ) -> Result<(), Error> {
        let mut digest_checked = false;
        if let Some(digest) = digest {
            if prior_lookup && self.lookup_digest(digest).await? {
                return Ok(());
            }
            digest_checked = true;
        }
        let augmented_manifest_envelope = manifest_id.load(ctx, blobstore).await?;
        let digest = MononokeDigest(
            augmented_manifest_envelope.augmented_manifest_id(),
            augmented_manifest_envelope.augmented_manifest_size(),
        );
        if !digest_checked && prior_lookup && self.lookup_digest(&digest).await? {
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
    /// Digests of trees can be known a priori (from Augmented Manifest, for example), but it is not required.
    /// Caller can analyse individual results.
    pub async fn upload_augmented_trees<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        manifest_ids: Vec<(HgAugmentedManifestId, Option<MononokeDigest>)>,
        prior_lookup: bool,
    ) -> Vec<Result<HgAugmentedManifestId, Error>> {
        let uploads = manifest_ids.into_iter().map(move |id| async move {
            self.upload_augmented_tree(ctx, blobstore, &id.0, id.1.as_ref(), prior_lookup)
                .await?;
            Ok(id.0)
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
    ) -> Result<Vec<HgAugmentedManifestId>, Error> {
        self.upload_augmented_trees(ctx, blobstore, manifest_ids, prior_lookup)
            .await
            .into_iter()
            .collect()
    }

    /// Upload given root augmented tree to a CAS backend (recursively by walking the tree)
    // TODO: rewrite it to use the "bounded_traversal" crate.
    pub async fn upload_root_augmented_tree_recursive<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        root_manifest_id: &HgAugmentedManifestId,
    ) -> Result<(), Error> {
        let mut manifests_queue = vec![(root_manifest_id.clone(), None)];
        let mut blobs_queue: Vec<(HgFileNodeId, Option<MononokeDigest>)> = Vec::new();

        while !manifests_queue.is_empty() {
            // If blobs queue is big, let's drain it first
            if blobs_queue.len() > MAX_CONCURRENT_UPLOADS_FILES {
                let uploaded_file_ids = self
                    .ensure_upload_file_contents(
                        ctx,
                        blobstore,
                        blobs_queue
                            .drain(0..MAX_CONCURRENT_UPLOADS_FILES)
                            .collect::<Vec<_>>(),
                        true,
                    )
                    .await?;

                info!(
                    ctx.logger(),
                    "Uploaded {} files into cas backend",
                    uploaded_file_ids.len()
                );
            }

            // Upload chunk of manifests from the current queue up to MAX_CONCURRENT_UPLOADS_TREES amount
            let uploaded_manifest_ids = self
                .ensure_upload_augmented_trees(
                    ctx,
                    blobstore,
                    manifests_queue
                        .drain(0..cmp::min(manifests_queue.len(), MAX_CONCURRENT_UPLOADS_TREES))
                        .collect::<Vec<_>>(),
                    true,
                )
                .await?;

            info!(
                ctx.logger(),
                "Uploaded {} augmented manifests into cas backend",
                uploaded_manifest_ids.len()
            );

            // Fetch all children of the uploaded manifests chunk
            let fetches = uploaded_manifest_ids
                .into_iter()
                .map(move |manifest_id| async move {
                    let manifest = manifest_id.load(ctx, blobstore).await?.augmented_manifest;
                    let childrens: Vec<HgAugmentedManifestEntry> = manifest
                        .into_subentries(ctx, blobstore)
                        // strip the paths
                        .map(|res| res.map(|(_k, v)| v))
                        .try_collect()
                        .await?;
                    Ok::<Vec<HgAugmentedManifestEntry>, Error>(childrens)
                });

            let children: Vec<HgAugmentedManifestEntry> = stream::iter(fetches)
                .buffer_unordered(MAX_CONCURRENT_UPLOADS_TREES)
                .try_collect::<Vec<Vec<HgAugmentedManifestEntry>>>()
                .await?
                .into_iter()
                .flatten()
                .collect();

            for child in children {
                match child {
                    HgAugmentedManifestEntry::FileNode(file) => {
                        blobs_queue.push((
                            HgFileNodeId::new(file.filenode),
                            Some(MononokeDigest(file.content_blake3, file.total_size)),
                        ));
                    }
                    HgAugmentedManifestEntry::DirectoryNode(tree) => {
                        manifests_queue.push((
                            HgAugmentedManifestId::new(tree.treenode),
                            Some(MononokeDigest(
                                tree.augmented_manifest_id,
                                tree.augmented_manifest_size,
                            )),
                        ));
                    }
                }
            }
        }

        // Upload remaining blobs
        let uploaded_file_ids = self
            .ensure_upload_file_contents(
                ctx,
                blobstore,
                blobs_queue.into_iter().collect::<Vec<_>>(),
                true,
            )
            .await?;

        info!(
            ctx.logger(),
            "Uploaded {} files into cas backend",
            uploaded_file_ids.len()
        );
        Ok(())
    }
}
