/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use ::manifest::Entry;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use futures::future;
use futures::future::BoxFuture;
use futures::future::Future;
use futures::future::FutureExt;
use futures::pin_mut;
use futures::stream;
use futures::stream::StreamExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use futures_stats::TimedTryFutureExt;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use slog::trace;
use slog::Logger;
use stats::prelude::*;
use time_ext::DurationExt;

use super::errors::MononokeHgBlobError;
use super::filenode_lookup::lookup_filenode_id;
use super::filenode_lookup::store_filenode_id;
use super::filenode_lookup::FileNodeIdPointer;
use super::File;
use super::META_SZ;
use crate::calculate_hg_node_id_stream;
use crate::FileBytes;
use crate::HgBlobNode;
use crate::HgFileEnvelopeMut;
use crate::HgFileNodeId;
use crate::HgManifestEnvelopeMut;
use crate::HgManifestId;
use crate::HgNodeHash;
use crate::HgParents;

define_stats! {
    prefix = "mononoke.blobrepo";
    upload_hg_file_entry: timeseries(Rate, Sum),
    upload_hg_tree_entry: timeseries(Rate, Sum),
    upload_blob: timeseries(Rate, Sum),
}

/// Metadata associated with a content blob being uploaded as part of changeset creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentBlobMeta {
    pub id: ContentId,
    pub size: u64,
    // The copy info will later be stored as part of the commit.
    pub copy_from: Option<(NonRootMPath, HgFileNodeId)>,
}

/// Node hash handling for upload entries
pub enum UploadHgNodeHash {
    /// Generate the hash from the uploaded content
    Generate,
    /// This hash is used as the blobstore key, even if it doesn't match the hash of the
    /// parents and raw content. This is done because in some cases like root tree manifests
    /// in hybrid mode, Mercurial sends fake hashes.
    Supplied(HgNodeHash),
    /// As Supplied, but Verify the supplied hash - if it's wrong, you will get an error.
    Checked(HgNodeHash),
}

/// Context for uploading a Mercurial manifest entry.
pub struct UploadHgTreeEntry {
    pub upload_node_id: UploadHgNodeHash,
    pub contents: Bytes,
    pub p1: Option<HgNodeHash>, // TODO: How hard is it to udpate those?
    pub p2: Option<HgNodeHash>,
    pub path: RepoPath,
}

impl UploadHgTreeEntry {
    // Given the content of a manifest, ensure that there is a matching Entry in the repo.
    // This may not upload the entry or the data blob if the repo is aware of that data already
    // existing in the underlying store.
    //
    // Note that the Entry may not be consistent - parents do not have to be uploaded at this
    // point, as long as you know their HgNodeHashes; this is also given to you as part of the
    // result type, so that you can parallelise uploads. Consistency will be verified when adding
    // the entries to a changeset.
    pub fn upload(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
    ) -> Result<(
        HgManifestId,
        BoxFuture<'static, Result<(HgManifestId, RepoPath)>>,
    )> {
        STATS::upload_hg_tree_entry.add_value(1);
        let UploadHgTreeEntry {
            upload_node_id,
            contents,
            p1,
            p2,
            path,
        } = self;

        let logger = ctx.logger().clone();
        let computed_node_id = HgBlobNode::new(contents.clone(), p1, p2).nodeid();
        let node_id: HgNodeHash = match upload_node_id {
            UploadHgNodeHash::Generate => computed_node_id,
            UploadHgNodeHash::Supplied(node_id) => node_id,
            UploadHgNodeHash::Checked(node_id) => {
                if node_id != computed_node_id {
                    bail!(MononokeHgBlobError::InconsistentEntryHashForPath(
                        path,
                        node_id,
                        computed_node_id
                    ));
                }
                node_id
            }
        };

        // This is the blob that gets uploaded. Manifest contents are usually small so they're
        // stored inline.
        let envelope = HgManifestEnvelopeMut {
            node_id,
            p1,
            p2,
            computed_node_id,
            contents,
        };
        let envelope_blob = envelope.freeze().into_blob();

        let manifest_id = HgManifestId::new(node_id);
        let blobstore_key = manifest_id.blobstore_key();

        fn log_upload_stats(
            logger: Logger,
            path: RepoPath,
            node_id: HgNodeHash,
            computed_node_id: HgNodeHash,
            stats: FutureStats,
        ) {
            trace!(logger, "Upload HgManifestEnvelope stats";
                "phase" => "manifest_envelope_uploaded".to_string(),
                "path" => format!("{}", path),
                "node_id" => format!("{}", node_id),
                "computed_node_id" => format!("{}", computed_node_id),
                "poll_count" => stats.poll_count,
                "poll_time_us" => stats.poll_time.as_micros_unchecked(),
                "completion_time_us" => stats.completion_time.as_micros_unchecked(),
            );
        }

        // Upload the blob.
        let upload = {
            let path = path.clone();
            let logger = logger.clone();
            async move {
                let (stats, ()) = blobstore
                    .put(&ctx, blobstore_key, envelope_blob.into())
                    .try_timed()
                    .await?;
                log_upload_stats(logger, path.clone(), node_id, computed_node_id, stats);
                Ok((manifest_id, path))
            }
        };

        Ok((manifest_id, upload.boxed()))
    }

    pub fn upload_as_entry(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
    ) -> Result<(
        HgManifestId,
        BoxFuture<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath)>>,
    )> {
        let (mfid, upload_fut) = self.upload(ctx, blobstore.clone())?;
        let upload_fut = async move {
            let (mfid, repo_path) = upload_fut.await?;
            Ok((Entry::Tree(mfid), repo_path))
        }
        .boxed();
        Ok((mfid, upload_fut))
    }
}

/// What sort of file contents are available to upload.
pub enum UploadHgFileContents {
    /// Content already uploaded (or scheduled to be uploaded). Metadata will be inlined in
    /// the envelope.
    ContentUploaded(ContentBlobMeta),
    /// Raw bytes as would be sent by Mercurial, including any metadata prepended in the standard
    /// Mercurial format.
    RawBytes(Bytes, FilestoreConfig),
}

impl UploadHgFileContents {
    /// Upload the file contents if necessary, and asynchronously return the hash of the file node
    /// and metadata.
    fn execute(
        self,
        ctx: CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> (
        ContentBlobMeta,
        // The future that does the upload and the future that computes the node ID/metadata are
        // split up to allow greater parallelism.
        impl Future<Output = Result<()>> + Send,
        impl Future<Output = Result<(HgFileNodeId, Bytes, u64)>> + Send,
    ) {
        let (cbmeta, upload_fut, compute_fut) = match self {
            UploadHgFileContents::ContentUploaded(cbmeta) => {
                let upload_fut = async move { Ok(()) };

                let size = cbmeta.size;

                let lookup_fut = {
                    cloned!(ctx, blobstore);
                    let file_node_id_ptr =
                        FileNodeIdPointer::new(&cbmeta.id, &cbmeta.copy_from, &p1, &p2);
                    async move {
                        lookup_filenode_id(&ctx, &blobstore, file_node_id_ptr)
                            .await
                            .context("failed to lookup filenode id")
                    }
                };

                let metadata_fut = Self::compute_metadata(
                    ctx.clone(),
                    blobstore,
                    cbmeta.id,
                    cbmeta.copy_from.clone(),
                );

                let content_id = cbmeta.id;

                // Attempt to lookup filenode ID by alias. Fallback to computing it if we cannot.
                let compute_fut = {
                    cloned!(ctx, blobstore);
                    async move {
                        let (res, metadata) = future::try_join(lookup_fut, metadata_fut).await?;

                        // Remember if this filenode pointer was a cache hit so that we can skip re-uploading it.
                        let (is_hit, fnid) = match res {
                            Some(fnid) => (true, fnid),
                            _ => {
                                let fnid = Self::compute_filenode_id(
                                    ctx,
                                    &blobstore,
                                    content_id,
                                    metadata.clone(),
                                    p1,
                                    p2,
                                )
                                .await?;
                                (false, fnid)
                            }
                        };
                        anyhow::Ok((is_hit, fnid, metadata, size))
                    }
                };

                (cbmeta, upload_fut.left_future(), compute_fut.left_future())
            }
            UploadHgFileContents::RawBytes(raw_content, filestore_config) => {
                let node_id = HgFileNodeId::new(
                    HgBlobNode::new(
                        raw_content.clone(),
                        p1.map(HgFileNodeId::into_nodehash),
                        p2.map(HgFileNodeId::into_nodehash),
                    )
                    .nodeid(),
                );

                let f = File::new(raw_content, p1, p2);
                let metadata = f.metadata();

                let copy_from = match f.copied_from() {
                    Ok(copy_from) => copy_from,
                    // XXX error out if copy-from information couldn't be read?
                    Err(_err) => None,
                };
                // Upload the contents separately (they'll be used for bonsai changesets as well).
                let file_bytes = f.file_contents();

                STATS::upload_blob.add_value(1);
                let ((id, size), upload_fut) = filestore::store_bytes(
                    blobstore,
                    filestore_config,
                    &ctx,
                    file_bytes.into_bytes(),
                );

                let upload_fut = {
                    let logger = ctx.logger().clone();
                    async move {
                        let (stats, result) = upload_fut.timed().await;
                        if result.is_ok() {
                            UploadHgFileEntry::log_stats(
                                logger,
                                None,
                                node_id,
                                "content_uploaded",
                                stats,
                            );
                        }
                        result
                    }
                };

                let cbmeta = ContentBlobMeta {
                    id,
                    size,
                    copy_from,
                };

                let compute_fut = async move { Ok((false, node_id, metadata, size)) };

                (
                    cbmeta,
                    upload_fut.right_future(),
                    compute_fut.right_future(),
                )
            }
        };

        let key = FileNodeIdPointer::new(&cbmeta.id, &cbmeta.copy_from, &p1, &p2);

        let compute_fut = {
            cloned!(ctx, blobstore);
            async move {
                let (is_hit, filenode_id, metadata, size) = compute_fut.await?;
                if !is_hit {
                    store_filenode_id(&ctx, &blobstore, key, &filenode_id).await?;
                }
                Ok((filenode_id, metadata, size))
            }
        };

        (cbmeta, upload_fut, compute_fut)
    }

    fn compute_metadata(
        ctx: CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        content_id: ContentId,
        copy_from: Option<(NonRootMPath, HgFileNodeId)>,
    ) -> impl Future<Output = Result<Bytes>> + Send {
        cloned!(blobstore);

        async move {
            let bytes =
                filestore::peek(&blobstore, &ctx, &FetchKey::Canonical(content_id), META_SZ)
                    .await?
                    .ok_or(MononokeHgBlobError::ContentBlobMissing(content_id))
                    .context("Failed to compute Hg file metadata")?;

            let mut metadata = Vec::new();
            File::generate_metadata(copy_from.as_ref(), &FileBytes(bytes), &mut metadata)
                .expect("Vec::write_all should never fail");

            Ok(Bytes::from(metadata))
        }
    }

    fn compute_filenode_id(
        ctx: CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        content_id: ContentId,
        metadata: Bytes,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> impl Future<Output = Result<HgFileNodeId>> + Send {
        cloned!(blobstore);
        async move {
            let file_bytes = async_stream::stream! {
                let stream = filestore::fetch(&blobstore, ctx, &FetchKey::Canonical(content_id))
                    .await?
                    .ok_or(MononokeHgBlobError::ContentBlobMissing(content_id))?;

                pin_mut!(stream);
                while let Some(value) = stream.next().await {
                    yield value;
                }
            };

            let all_bytes = stream::once(async move { anyhow::Ok(metadata) }).chain(file_bytes);

            let hg_parents = HgParents::new(
                p1.map(HgFileNodeId::into_nodehash),
                p2.map(HgFileNodeId::into_nodehash),
            );

            let node_hash = calculate_hg_node_id_stream(all_bytes, &hg_parents)
                .await
                .context("Failed to compute Hg filenode id")?;

            Ok(HgFileNodeId::new(node_hash))
        }
    }
}

/// Context for uploading a Mercurial file entry.
pub struct UploadHgFileEntry {
    pub upload_node_id: UploadHgNodeHash,
    pub contents: UploadHgFileContents,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
}

impl UploadHgFileEntry {
    pub async fn upload(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        path: Option<&NonRootMPath>, // This is used for logging
    ) -> Result<HgFileNodeId, Error> {
        STATS::upload_hg_file_entry.add_value(1);
        let UploadHgFileEntry {
            upload_node_id,
            contents,
            p1,
            p2,
        } = self;

        let (cbmeta, content_upload, compute_fut) =
            contents.execute(ctx.clone(), &blobstore, p1, p2);
        let content_id = cbmeta.id;
        let logger = ctx.logger().clone();

        let envelope_upload = async move {
            let (computed_node_id, metadata, content_size) = compute_fut.await?;

            let node_id = match upload_node_id {
                UploadHgNodeHash::Generate => computed_node_id,
                UploadHgNodeHash::Supplied(node_id) => HgFileNodeId::new(node_id),
                UploadHgNodeHash::Checked(node_id) => {
                    let node_id = HgFileNodeId::new(node_id);
                    if node_id != computed_node_id {
                        let err = match path {
                            Some(path) => MononokeHgBlobError::InconsistentEntryHashForPath(
                                RepoPath::FilePath((*path).clone()),
                                node_id.into_nodehash(),
                                computed_node_id.into_nodehash(),
                            ),
                            None => MononokeHgBlobError::InconsistentEntryHash(
                                node_id.into_nodehash(),
                                computed_node_id.into_nodehash(),
                            ),
                        };
                        return Err(Error::from(err));
                    }
                    node_id
                }
            };

            let file_envelope = HgFileEnvelopeMut {
                node_id,
                p1,
                p2,
                content_id,
                content_size,
                metadata,
            };
            let envelope_blob = file_envelope.freeze().into_blob();

            let blobstore_key = node_id.blobstore_key();

            let (stats, ()) = blobstore
                .put(&ctx, blobstore_key, envelope_blob.into())
                .try_timed()
                .await?;

            Self::log_stats(logger, path, node_id, "file_envelope_uploaded", stats);

            Ok(node_id)
        };

        let (ret, ()) = future::try_join(envelope_upload, content_upload).await?;

        Ok(ret)
    }

    /// Upload and provide a path for this upload. This will be returned to the caller, but wil
    /// also be used for logging.
    pub async fn upload_with_path(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        path: NonRootMPath,
    ) -> Result<(HgFileNodeId, RepoPath), Error> {
        let filenode_id = self.upload(ctx, blobstore.clone(), Some(&path)).await?;
        Ok((filenode_id, RepoPath::FilePath(path)))
    }

    pub async fn upload_as_entry(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        path: NonRootMPath,
    ) -> Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath), Error> {
        let filenode_id = self.upload(ctx, blobstore.clone(), Some(&path)).await?;
        Ok((Entry::Leaf(filenode_id), RepoPath::FilePath(path)))
    }

    fn log_stats(
        logger: Logger,
        path: Option<&NonRootMPath>,
        nodeid: HgFileNodeId,
        phase: &str,
        stats: FutureStats,
    ) {
        let path = path.map_or_else(String::new, |p| p.to_string());
        let nodeid = format!("{}", nodeid);
        trace!(logger, "Upload blob stats";
            "phase" => String::from(phase),
            "path" => path,
            "nodeid" => nodeid,
            "poll_count" => stats.poll_count,
            "poll_time_us" => stats.poll_time.as_micros_unchecked(),
            "completion_time_us" => stats.completion_time.as_micros_unchecked(),
        );
    }
}
