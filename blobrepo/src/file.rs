// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Plain files, symlinks

use crate::envelope::HgBlobEnvelope;
use crate::errors::*;
use crate::manifest::{fetch_manifest_envelope, fetch_raw_manifest_bytes, BlobManifest};
use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{bail_msg, Error, FutureFailureErrorExt, StreamFailureErrorExt};
use filestore::{self, FetchKey};
use futures::{
    future::{lazy, Future},
    stream::Stream,
};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use mercurial_types::{
    calculate_hg_node_id,
    manifest::{Content, HgEntry, HgManifest, Type},
    nodehash::HgEntryId,
    FileBytes, FileType, HgBlob, HgFileEnvelope, HgFileNodeId, HgManifestId, HgNodeHash, HgParents,
    MPathElement,
};
use mononoke_types::{hash::Sha256, ContentId, ContentMetadata};
use std::sync::Arc;

#[derive(Clone)]
pub struct HgBlobEntry {
    blobstore: Arc<dyn Blobstore>,
    name: Option<MPathElement>,
    id: HgEntryId,
}

impl PartialEq for HgBlobEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.id == other.id
    }
}

impl Eq for HgBlobEntry {}

pub fn fetch_raw_filenode_bytes(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
    validate_hash: bool,
) -> BoxFuture<HgBlob, Error> {
    fetch_file_envelope(ctx.clone(), blobstore, node_id)
        .and_then({
            let blobstore = blobstore.clone();
            move |envelope| {
                let envelope = envelope.into_mut();

                // TODO (T47717165): Avoid buffering here.
                let file_bytes_fut =
                    fetch_file_contents(ctx, &blobstore, envelope.content_id).concat2();

                let mut metadata = envelope.metadata;
                let f = if metadata.is_empty() {
                    file_bytes_fut
                        .map(|contents| contents.into_bytes())
                        .left_future()
                } else {
                    file_bytes_fut
                        .map(move |contents| {
                            // The copy info and the blob have to be joined together.
                            // TODO (T30456231): avoid the copy
                            metadata.extend_from_slice(contents.into_bytes().as_ref());
                            metadata
                        })
                        .right_future()
                };

                let p1 = envelope.p1.map(|p| p.into_nodehash());
                let p2 = envelope.p2.map(|p| p.into_nodehash());
                f.and_then(move |content| {
                    if validate_hash {
                        let actual = HgFileNodeId::new(calculate_hg_node_id(
                            &content,
                            &HgParents::new(p1, p2),
                        ));

                        if actual != node_id {
                            return Err(ErrorKind::CorruptHgFileNode {
                                expected: node_id,
                                actual,
                            }
                            .into());
                        }
                    }
                    Ok(content)
                })
                .map(HgBlob::from)
            }
        })
        .from_err()
        .boxify()
}

pub fn fetch_file_content_from_blobstore(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
) -> impl Stream<Item = FileBytes, Error = Error> {
    fetch_file_envelope(ctx.clone(), blobstore, node_id)
        .map({
            cloned!(blobstore);
            move |envelope| {
                let content_id = envelope.content_id();
                fetch_file_contents(ctx, &blobstore, content_id.clone())
            }
        })
        .flatten_stream()
}

pub fn fetch_file_size_from_blobstore(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
) -> impl Future<Item = u64, Error = Error> {
    fetch_file_envelope(ctx, blobstore, node_id).map({ |envelope| envelope.content_size() })
}

pub fn fetch_file_content_id_from_blobstore(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
) -> impl Future<Item = ContentId, Error = Error> {
    fetch_file_envelope(ctx, blobstore, node_id).map({ |envelope| envelope.content_id() })
}

pub fn fetch_file_metadata_from_blobstore(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    content_id: ContentId,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    filestore::get_metadata(blobstore, ctx, &FetchKey::Canonical(content_id))
        .and_then(move |aliases| aliases.ok_or(ErrorKind::ContentBlobMissing(content_id).into()))
        .context("While fetching content metadata")
        .from_err()
}

pub fn fetch_file_content_sha256_from_blobstore(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    content_id: ContentId,
) -> impl Future<Item = Sha256, Error = Error> {
    fetch_file_metadata_from_blobstore(ctx, blobstore, content_id).map(|metadata| metadata.sha256)
}

pub fn fetch_file_parents_from_blobstore(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
) -> impl Future<Item = HgParents, Error = Error> {
    fetch_file_envelope(ctx, blobstore, node_id).map(|envelope| {
        let envelope = envelope.into_mut();
        let p1 = envelope.p1.map(|filenode| filenode.into_nodehash());
        let p2 = envelope.p2.map(|filenode| filenode.into_nodehash());
        HgParents::new(p1, p2)
    })
}

pub fn fetch_file_envelope(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
) -> impl Future<Item = HgFileEnvelope, Error = Error> {
    fetch_file_envelope_opt(ctx, blobstore, node_id)
        .and_then(move |envelope| {
            let envelope = envelope.ok_or(ErrorKind::HgContentMissing(
                node_id.into_nodehash(),
                Type::File(FileType::Regular),
            ))?;
            Ok(envelope)
        })
        .from_err()
}

pub fn fetch_file_envelope_opt(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
) -> impl Future<Item = Option<HgFileEnvelope>, Error = Error> {
    let blobstore_key = node_id.blobstore_key();
    blobstore
        .get(ctx, blobstore_key.clone())
        .context("While fetching manifest envelope blob")
        .map_err(Error::from)
        .and_then(move |bytes| {
            let blobstore_bytes = match bytes {
                Some(bytes) => bytes,
                None => return Ok(None),
            };
            let envelope = HgFileEnvelope::from_blob(blobstore_bytes.into())?;
            if node_id != envelope.node_id() {
                bail_msg!(
                    "Manifest ID mismatch (requested: {}, got: {})",
                    node_id,
                    envelope.node_id()
                );
            }
            Ok(Some(envelope))
        })
        .with_context(|_| ErrorKind::FileNodeDeserializeFailed(blobstore_key))
        .from_err()
}

pub fn fetch_file_contents(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    content_id: ContentId,
) -> impl Stream<Item = FileBytes, Error = Error> {
    filestore::fetch(blobstore, ctx, &FetchKey::Canonical(content_id))
        .and_then(move |stream| stream.ok_or(ErrorKind::ContentBlobMissing(content_id).into()))
        .flatten_stream()
        .map(FileBytes)
        .context("While fetching content blob")
        .from_err()
}

impl HgBlobEntry {
    pub fn new(
        blobstore: Arc<dyn Blobstore>,
        name: MPathElement,
        nodeid: HgNodeHash,
        ty: Type,
    ) -> Self {
        Self {
            blobstore,
            name: Some(name),
            id: match ty {
                Type::Tree => HgEntryId::Manifest(HgManifestId::new(nodeid)),
                Type::File(file_type) => HgEntryId::File(file_type, HgFileNodeId::new(nodeid)),
            },
        }
    }

    pub fn new_root(blobstore: Arc<dyn Blobstore>, manifestid: HgManifestId) -> Self {
        Self {
            blobstore,
            name: None,
            id: manifestid.into(),
        }
    }

    fn get_raw_content_inner(&self, ctx: CoreContext) -> BoxFuture<HgBlob, Error> {
        let validate_hash = false;
        match self.id {
            HgEntryId::Manifest(manifest_id) => {
                fetch_raw_manifest_bytes(ctx, &self.blobstore, manifest_id)
            }
            HgEntryId::File(_, filenode_id) => {
                // TODO (torozco) T48791324: Identify if get_raw_content is being used at all on
                // filenodes, and remove callers so we can remove it. As-is, if called, this could
                // try to access arbitrarily large files.
                fetch_raw_filenode_bytes(ctx, &self.blobstore, filenode_id, validate_hash)
            }
        }
    }

    pub fn get_envelope(&self, ctx: CoreContext) -> BoxFuture<Box<dyn HgBlobEnvelope>, Error> {
        match self.id {
            HgEntryId::Manifest(hash) => fetch_manifest_envelope(ctx, &self.blobstore, hash)
                .map(|e| Box::new(e) as Box<dyn HgBlobEnvelope>)
                .left_future(),
            HgEntryId::File(_, hash) => fetch_file_envelope(ctx, &self.blobstore, hash)
                .map(|e| Box::new(e) as Box<dyn HgBlobEnvelope>)
                .right_future(),
        }
        .boxify()
    }
}

impl HgEntry for HgBlobEntry {
    fn get_type(&self) -> Type {
        self.id.get_type()
    }

    fn get_parents(&self, ctx: CoreContext) -> BoxFuture<HgParents, Error> {
        self.get_envelope(ctx).map(|e| e.get_parents()).boxify()
    }

    fn get_raw_content(&self, ctx: CoreContext) -> BoxFuture<HgBlob, Error> {
        self.get_raw_content_inner(ctx)
    }

    fn get_content(&self, ctx: CoreContext) -> BoxFuture<Content, Error> {
        let blobstore = self.blobstore.clone();

        let id = self.id.clone();
        let name = self.name.clone();
        // Note: do not remove `lazy(|| ...)` below! It helps with memory usage on serving
        // gettreepack requests.
        match self.id {
            HgEntryId::Manifest(manifest_id) => lazy(move || {
                BlobManifest::load(ctx, &blobstore, manifest_id)
                    .and_then({
                        move |blob_manifest| {
                            let manifest = blob_manifest.ok_or(ErrorKind::HgContentMissing(
                                id.into_nodehash(),
                                Type::Tree,
                            ))?;
                            Ok(Content::Tree(manifest.boxed()))
                        }
                    })
                    .context(format!(
                        "While HgBlobEntry::get_content for id {}, name {:?}",
                        id, name,
                    ))
                    .from_err()
            })
            .boxify(),
            HgEntryId::File(file_type, filenode_id) => lazy(move || {
                fetch_file_envelope(ctx.clone(), &blobstore, filenode_id)
                    .map(move |envelope| {
                        let envelope = envelope.into_mut();
                        let stream =
                            fetch_file_contents(ctx, &blobstore, envelope.content_id).boxify();

                        match file_type {
                            FileType::Regular => Content::File(stream),
                            FileType::Executable => Content::Executable(stream),
                            FileType::Symlink => Content::Symlink(stream),
                        }
                    })
                    .context(format!(
                        "While HgBlobEntry::get_content for id {}, name {:?}",
                        id, name
                    ))
                    .from_err()
            })
            .boxify(),
        }
    }

    fn get_size(&self, ctx: CoreContext) -> BoxFuture<Option<u64>, Error> {
        self.get_envelope(ctx).map(|e| e.get_size()).boxify()
    }

    fn get_hash(&self) -> HgEntryId {
        self.id
    }

    fn get_name(&self) -> Option<&MPathElement> {
        self.name.as_ref()
    }
}
