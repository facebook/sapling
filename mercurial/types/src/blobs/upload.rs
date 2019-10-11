/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::filenode_lookup::{lookup_filenode_id, store_filenode_id, FileNodeIdPointer};
use super::{errors::ErrorKind, File, HgBlobEntry, META_SZ};
use crate::{
    calculate_hg_node_id_stream, FileBytes, HgBlobNode, HgFileEnvelopeMut, HgFileNodeId,
    HgManifestEnvelopeMut, HgManifestId, HgNodeHash, HgParents, Type,
};
use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use failure_ext::{bail_err, FutureFailureErrorExt, Result};
use filestore::{self, FetchKey};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use futures_stats::{FutureStats, Timed};
use mononoke_types::{ContentId, FileType, MPath, RepoPath};
use slog::{trace, Logger};
use stats::{define_stats, Timeseries};
use std::sync::Arc;
use time_ext::DurationExt;

define_stats! {
    prefix = "mononoke.blobrepo";
    upload_hg_file_entry: timeseries(RATE, SUM),
    upload_hg_tree_entry: timeseries(RATE, SUM),
    upload_blob: timeseries(RATE, SUM),
}

/// Information about a content blob associated with a push that is available in
/// the blobstore. (This blob wasn't necessarily uploaded in this push.)
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentBlobInfo {
    pub path: MPath,
    pub meta: ContentBlobMeta,
}

/// Metadata associated with a content blob being uploaded as part of changeset creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentBlobMeta {
    pub id: ContentId,
    pub size: u64,
    // The copy info will later be stored as part of the commit.
    pub copy_from: Option<(MPath, HgFileNodeId)>,
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
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub path: RepoPath,
}

impl UploadHgTreeEntry {
    // Given the content of a manifest, ensure that there is a matching HgBlobEntry in the repo.
    // This may not upload the entry or the data blob if the repo is aware of that data already
    // existing in the underlying store.
    //
    // Note that the HgBlobEntry may not be consistent - parents do not have to be uploaded at this
    // point, as long as you know their HgNodeHashes; this is also given to you as part of the
    // result type, so that you can parallelise uploads. Consistency will be verified when
    // adding the entries to a changeset.
    // adding the entries to a changeset.
    pub fn upload(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
    ) -> Result<(HgNodeHash, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
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
                    bail_err!(ErrorKind::InconsistentEntryHash(
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

        let blob_entry = match path.mpath().and_then(|m| m.into_iter().last()) {
            Some(m) => {
                let entry_path = m.clone();
                HgBlobEntry::new(blobstore.clone(), entry_path, node_id, Type::Tree)
            }
            None => HgBlobEntry::new_root(blobstore.clone(), manifest_id),
        };

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
        let upload = blobstore
            .put(ctx, blobstore_key, envelope_blob.into())
            .map({
                let path = path.clone();
                move |()| (blob_entry, path)
            })
            .timed({
                let logger = logger.clone();
                move |stats, result| {
                    if result.is_ok() {
                        log_upload_stats(logger, path, node_id, computed_node_id, stats);
                    }
                    Ok(())
                }
            });

        Ok((node_id, upload.boxify()))
    }
}

/// What sort of file contents are available to upload.
pub enum UploadHgFileContents {
    /// Content already uploaded (or scheduled to be uploaded). Metadata will be inlined in
    /// the envelope.
    ContentUploaded(ContentBlobMeta),
    /// Raw bytes as would be sent by Mercurial, including any metadata prepended in the standard
    /// Mercurial format.
    RawBytes(Bytes),
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
        path: MPath,
    ) -> (
        ContentBlobInfo,
        // The future that does the upload and the future that computes the node ID/metadata are
        // split up to allow greater parallelism.
        impl Future<Item = (), Error = Error> + Send,
        impl Future<Item = (HgFileNodeId, Bytes, u64), Error = Error> + Send,
    ) {
        let (cbinfo, upload_fut, compute_fut) = match self {
            UploadHgFileContents::ContentUploaded(cbmeta) => {
                let upload_fut = future::ok(());

                let size = cbmeta.size;
                let cbinfo = ContentBlobInfo { path, meta: cbmeta };

                let lookup_fut = lookup_filenode_id(
                    ctx.clone(),
                    &*blobstore,
                    FileNodeIdPointer::new(&cbinfo.meta.id, &cbinfo.meta.copy_from, &p1, &p2),
                );

                let metadata_fut = Self::compute_metadata(
                    ctx.clone(),
                    blobstore,
                    cbinfo.meta.id,
                    cbinfo.meta.copy_from.clone(),
                );

                let content_id = cbinfo.meta.id;

                // Attempt to lookup filenode ID by alias. Fallback to computing it if we cannot.
                let compute_fut = (lookup_fut, metadata_fut).into_future().and_then({
                    cloned!(ctx, blobstore);
                    move |(res, metadata)| {
                        res.ok_or(())
                            .into_future()
                            .or_else({
                                cloned!(metadata);
                                move |_| {
                                    Self::compute_filenode_id(
                                        ctx, &blobstore, content_id, metadata, p1, p2,
                                    )
                                }
                            })
                            .map(move |fnid| (fnid, metadata, size))
                    }
                });

                (cbinfo, upload_fut.left_future(), compute_fut.left_future())
            }
            UploadHgFileContents::RawBytes(raw_content) => {
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
                let (contents, upload_fut) =
                    filestore::store_bytes(blobstore.clone(), ctx.clone(), file_bytes.into_bytes());

                let upload_fut = upload_fut.timed({
                    cloned!(path);
                    let logger = ctx.logger().clone();
                    move |stats, result| {
                        if result.is_ok() {
                            UploadHgFileEntry::log_stats(
                                logger,
                                path,
                                node_id,
                                "content_uploaded",
                                stats,
                            );
                        }
                        Ok(())
                    }
                });

                let id = contents.content_id();
                let size = contents.size();

                let cbinfo = ContentBlobInfo {
                    path,
                    meta: ContentBlobMeta {
                        id,
                        size,
                        copy_from,
                    },
                };

                let compute_fut = future::ok((node_id, metadata, size));

                (
                    cbinfo,
                    upload_fut.right_future(),
                    compute_fut.right_future(),
                )
            }
        };

        let key = FileNodeIdPointer::new(&cbinfo.meta.id, &cbinfo.meta.copy_from, &p1, &p2);

        let compute_fut = compute_fut.and_then({
            cloned!(ctx, blobstore);
            move |(filenode_id, metadata, size)| {
                store_filenode_id(ctx, &blobstore, key, &filenode_id)
                    .map(move |_| (filenode_id, metadata, size))
            }
        });

        (cbinfo, upload_fut, compute_fut)
    }

    fn compute_metadata(
        ctx: CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        content_id: ContentId,
        copy_from: Option<(MPath, HgFileNodeId)>,
    ) -> impl Future<Item = Bytes, Error = Error> {
        filestore::peek(&*blobstore, ctx, &FetchKey::Canonical(content_id), META_SZ)
            .and_then(move |bytes| bytes.ok_or(ErrorKind::ContentBlobMissing(content_id).into()))
            .context("While computing metadata")
            .from_err()
            .map(move |bytes| {
                let mut metadata = Vec::new();
                File::generate_metadata(copy_from.as_ref(), &FileBytes(bytes), &mut metadata)
                    .expect("Vec::write_all should never fail");

                // TODO: Introduce Metadata bytes?
                Bytes::from(metadata)
            })
    }

    fn compute_filenode_id(
        ctx: CoreContext,
        blobstore: &Arc<dyn Blobstore>,
        content_id: ContentId,
        metadata: Bytes,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> impl Future<Item = HgFileNodeId, Error = Error> {
        let file_bytes = filestore::fetch(&*blobstore, ctx, &FetchKey::Canonical(content_id))
            .and_then(move |stream| stream.ok_or(ErrorKind::ContentBlobMissing(content_id).into()))
            .flatten_stream();

        let all_bytes = stream::once(Ok(metadata)).chain(file_bytes);

        let hg_parents = HgParents::new(
            p1.map(HgFileNodeId::into_nodehash),
            p2.map(HgFileNodeId::into_nodehash),
        );

        calculate_hg_node_id_stream(all_bytes, &hg_parents)
            .map(HgFileNodeId::new)
            .context("While computing a filenode id")
            .from_err()
    }
}

/// Context for uploading a Mercurial file entry.
pub struct UploadHgFileEntry {
    pub upload_node_id: UploadHgNodeHash,
    pub contents: UploadHgFileContents,
    pub file_type: FileType,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub path: MPath,
}

impl UploadHgFileEntry {
    pub fn upload(
        self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
    ) -> Result<(ContentBlobInfo, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
        STATS::upload_hg_file_entry.add_value(1);
        let UploadHgFileEntry {
            upload_node_id,
            contents,
            file_type,
            p1,
            p2,
            path,
        } = self;

        let (cbinfo, content_upload, compute_fut) =
            contents.execute(ctx.clone(), &blobstore, p1, p2, path.clone());
        let content_id = cbinfo.meta.id;
        let logger = ctx.logger().clone();

        let envelope_upload =
            compute_fut.and_then(move |(computed_node_id, metadata, content_size)| {
                let node_id = match upload_node_id {
                    UploadHgNodeHash::Generate => computed_node_id,
                    UploadHgNodeHash::Supplied(node_id) => HgFileNodeId::new(node_id),
                    UploadHgNodeHash::Checked(node_id) => {
                        let node_id = HgFileNodeId::new(node_id);
                        if node_id != computed_node_id {
                            return future::err(
                                ErrorKind::InconsistentEntryHash(
                                    RepoPath::FilePath(path),
                                    node_id.into_nodehash(),
                                    computed_node_id.into_nodehash(),
                                )
                                .into(),
                            )
                            .left_future();
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

                let blob_entry = HgBlobEntry::new(
                    blobstore.clone(),
                    path.basename().clone(),
                    node_id.into_nodehash(),
                    Type::File(file_type),
                );

                blobstore
                    .put(ctx, blobstore_key, envelope_blob.into())
                    .timed({
                        let path = path.clone();
                        move |stats, result| {
                            if result.is_ok() {
                                Self::log_stats(
                                    logger,
                                    path,
                                    node_id,
                                    "file_envelope_uploaded",
                                    stats,
                                );
                            }
                            Ok(())
                        }
                    })
                    .map(move |()| (blob_entry, RepoPath::FilePath(path)))
                    .right_future()
            });

        let fut = envelope_upload
            .join(content_upload)
            .map(move |(envelope_res, ())| envelope_res);
        Ok((cbinfo, fut.boxify()))
    }

    fn log_stats(
        logger: Logger,
        path: MPath,
        nodeid: HgFileNodeId,
        phase: &str,
        stats: FutureStats,
    ) {
        let path = format!("{}", path);
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
