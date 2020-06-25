/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod redaction;

use std::{collections::HashSet, fmt};

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::file_history::get_file_history_maybe_incomplete;
use blobstore::Loadable;
use bytes::{Bytes, BytesMut};
use cloned::cloned;
use context::CoreContext;
use filestore::FetchKey;
use futures::future::TryFutureExt;
use futures_ext::{select_all, BoxFuture, FutureExt};
use futures_old::{Future, IntoFuture, Stream};
use getbundle_response::SessionLfsParams;
use mercurial_types::{
    blobs::File, calculate_hg_node_id, FileBytes, HgFileEnvelopeMut, HgFileHistoryEntry,
    HgFileNodeId, HgParents, MPath, RevFlags,
};
use revisionstore_types::Metadata;
use thiserror::Error;

use redaction::RedactionFutureExt;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Corrupt hg filenode returned: {expected} != {actual}")]
    CorruptHgFileNode {
        expected: HgFileNodeId,
        actual: HgFileNodeId,
    },

    #[error("Invalid blob kind returned: {kind:?}")]
    InvalidKind { kind: RemotefilelogBlobKind },
    #[error("Missing content: {0:?}")]
    MissingContent(FetchKey),
}

#[derive(Debug)]
pub enum RemotefilelogBlobKind {
    /// An inline filenode. This represents its size.
    Inline(u64),
    /// An LFS filenode.
    Lfs,
}

struct RemotefilelogBlob {
    kind: RemotefilelogBlobKind,
    /// data is a future of the metadata bytes and file bytes. For LFS blobs, the metadata bytes
    /// will be empty and the file bytes will contain a serialized LFS pointer.
    data: BoxFuture<(Bytes, FileBytes), Error>,
}

impl fmt::Debug for RemotefilelogBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RemotefilelogBlob {{ kind: {:?} }}", self.kind)
    }
}

/// Create a blob for getpack v1. This returns a future that resolves with an estimated weight for
/// this blob (this is NOT trying to be correct, it's just a rough estimate!), and the blob's
/// bytes.
pub fn create_getpack_v1_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    validate_hash: bool,
) -> impl Future<
    Item = (
        u64,
        impl Future<Item = (HgFileNodeId, Bytes), Error = Error>,
    ),
    Error = Error,
> {
    prepare_blob(
        ctx,
        repo,
        node,
        SessionLfsParams { threshold: None },
        validate_hash,
    )
    .map(move |RemotefilelogBlob { kind, data }| {
        use RemotefilelogBlobKind::*;

        let weight = match kind {
            Inline(size) => size,
            Lfs => unreachable!(), // lfs_threshold = None implies no LFS blobs.
        };

        let fut = data.rescue_redacted().map(move |(meta_bytes, file_bytes)| {
            // TODO (T30456231): Avoid this copy
            let mut buff = BytesMut::with_capacity(meta_bytes.len() + file_bytes.as_bytes().len());
            buff.extend_from_slice(&meta_bytes);
            buff.extend_from_slice(file_bytes.as_bytes());
            (node, buff.freeze())
        });

        (weight, fut)
    })
}

/// Create a blob for getpack v2. See v1 above for general details. This also returns Metadata,
/// which is present in the v2 version of the protocol.
pub fn create_getpack_v2_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    lfs_params: SessionLfsParams,
    validate_hash: bool,
) -> impl Future<
    Item = (
        u64,
        impl Future<Item = (HgFileNodeId, Bytes, Metadata), Error = Error>,
    ),
    Error = Error,
> {
    prepare_blob(ctx, repo, node, lfs_params, validate_hash).map(
        move |RemotefilelogBlob { kind, data }| {
            use RemotefilelogBlobKind::*;

            let (weight, metadata) = match kind {
                Inline(size) => (
                    size,
                    Metadata {
                        size: None,
                        flags: None,
                    },
                ),
                Lfs => {
                    let flags = Some(RevFlags::REVIDX_EXTSTORED.into());
                    (0, Metadata { size: None, flags })
                }
            };

            let fut = data.rescue_redacted().map(move |(meta_bytes, file_bytes)| {
                // TODO (T30456231): Avoid this copy
                let mut buff =
                    BytesMut::with_capacity(meta_bytes.len() + file_bytes.as_bytes().len());
                buff.extend_from_slice(&meta_bytes);
                buff.extend_from_slice(file_bytes.as_bytes());
                (node, buff.freeze(), metadata)
            });

            (weight, fut)
        },
    )
}

/// Retrieve the raw contents of a filenode. This does not substitute redacted content
/// (it'll just let the redacted error fall through).
pub fn create_raw_filenode_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    validate_hash: bool,
) -> impl Future<Item = Bytes, Error = Error> {
    prepare_blob(
        ctx,
        repo,
        node,
        SessionLfsParams { threshold: None },
        validate_hash,
    )
    .and_then(|RemotefilelogBlob { kind, data }| {
        use RemotefilelogBlobKind::*;

        match kind {
            Inline(_) => data.left_future(),
            kind @ _ => Err(ErrorKind::InvalidKind { kind }.into())
                .into_future()
                .right_future(),
        }
    })
    .map(|(meta_bytes, file_bytes)| {
        // TODO (T30456231): Avoid this copy
        let mut buff = BytesMut::with_capacity(meta_bytes.len() + file_bytes.as_bytes().len());
        buff.extend_from_slice(&meta_bytes);
        buff.extend_from_slice(file_bytes.as_bytes());
        buff.freeze()
    })
}

/// Get ancestors of all filenodes
/// Current implementation might be inefficient because it might re-fetch the same filenode a few
/// times
pub fn get_unordered_file_history_for_multiple_nodes(
    ctx: CoreContext,
    repo: BlobRepo,
    filenodes: HashSet<HgFileNodeId>,
    path: &MPath,
) -> impl Stream<Item = HgFileHistoryEntry, Error = Error> {
    select_all(filenodes.into_iter().map(|filenode| {
        get_file_history_maybe_incomplete(ctx.clone(), repo.clone(), filenode, path.clone(), None)
    }))
    .filter({
        let mut used_filenodes = HashSet::new();
        move |entry| used_filenodes.insert(entry.filenode().clone())
    })
}

fn prepare_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    lfs_params: SessionLfsParams,
    validate_hash: bool,
) -> impl Future<Item = RemotefilelogBlob, Error = Error> {
    node.load(ctx.clone(), repo.blobstore())
        .compat()
        .from_err()
        .map({
            cloned!(repo);
            move |envelope| {
                let file_size = envelope.content_size();

                let inline_file = match lfs_params.threshold {
                    Some(lfs_threshold) => (file_size <= lfs_threshold),
                    None => true,
                };

                // NOTE: It'd be nice if we could hoist up redaction checks to this point. Doing so
                // would let us return a different kind based on whether the content is redacted or
                // not, and therefore would make it more obvious which methods do redaction or not
                // (based on their signature).

                if inline_file {
                    let content_fut =
                        filestore::fetch_concat(repo.blobstore(), ctx, envelope.content_id())
                            .map(FileBytes);

                    let blob_fut = if validate_hash {
                        content_fut
                            .and_then(move |file_bytes| {
                                let HgFileEnvelopeMut {
                                    p1, p2, metadata, ..
                                } = envelope.into_mut();

                                let mut validation_bytes = BytesMut::with_capacity(
                                    metadata.len() + file_bytes.as_bytes().len(),
                                );
                                validation_bytes.extend_from_slice(&metadata);
                                validation_bytes.extend_from_slice(file_bytes.as_bytes());

                                let p1 = p1.map(|p| p.into_nodehash());
                                let p2 = p2.map(|p| p.into_nodehash());
                                let actual = HgFileNodeId::new(calculate_hg_node_id(
                                    &validation_bytes.freeze(),
                                    &HgParents::new(p1, p2),
                                ));

                                if actual != node {
                                    return Err(ErrorKind::CorruptHgFileNode {
                                        expected: node,
                                        actual,
                                    }
                                    .into());
                                }

                                Ok((metadata, file_bytes))
                            })
                            .boxify()
                    } else {
                        content_fut
                            .map(move |file_bytes| (envelope.into_mut().metadata, file_bytes))
                            .boxify()
                    };

                    RemotefilelogBlob {
                        kind: RemotefilelogBlobKind::Inline(file_size),
                        data: blob_fut,
                    }
                } else {
                    // For LFS blobs, we'll create the LFS pointer. Note that there is no hg-style
                    // metadata encoded for LFS blobs (it's in the LFS pointer instead).
                    let key = FetchKey::from(envelope.content_id());
                    let blob_fut = (
                        filestore::get_metadata(repo.blobstore(), ctx, &key).and_then(
                            move |meta| Ok(meta.ok_or(ErrorKind::MissingContent(key))?.sha256),
                        ),
                        File::extract_copied_from(envelope.metadata()).into_future(),
                    )
                        .into_future()
                        .and_then(move |(oid, copy_from)| {
                            File::generate_lfs_file(oid, file_size, copy_from)
                        })
                        .map(|bytes| (Bytes::new(), FileBytes(bytes)))
                        .boxify();

                    RemotefilelogBlob {
                        kind: RemotefilelogBlobKind::Lfs,
                        data: blob_fut,
                    }
                }
            }
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;
    use blobrepo_hg::BlobRepoHg;
    use blobrepo_override::DangerousOverride;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use futures::compat::Future01CompatExt;
    use manifest::{Entry, Manifest};
    use mononoke_types::MPathElement;
    use tests_utils::CreateCommitContext;

    async fn roundtrip_blob(
        fb: FacebookInit,
        repo: &BlobRepo,
        content: &str,
    ) -> Result<RemotefilelogBlobKind, Error> {
        let filename = "f1";

        let ctx = CoreContext::test_mock(fb);

        let bcs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(filename, content)
            .commit()
            .await?;

        let hg_manifest = repo
            .get_hg_from_bonsai_changeset(ctx.clone(), bcs)
            .compat()
            .await?
            .load(ctx.clone(), repo.blobstore())
            .await?
            .manifestid()
            .load(ctx.clone(), repo.blobstore())
            .await?;

        let entry = hg_manifest
            .lookup(&MPathElement::new(filename.as_bytes().to_vec())?)
            .ok_or(Error::msg("file is missing"))?;

        let filenode = match entry {
            Entry::Leaf((_, filenode)) => filenode,
            _ => {
                return Err(Error::msg("file is not a leaf"));
            }
        };

        let blob = prepare_blob(
            ctx.clone(),
            repo.clone(),
            filenode,
            SessionLfsParams { threshold: None },
            true,
        )
        .compat()
        .await?;

        let RemotefilelogBlob { kind, data } = blob;
        data.compat().await?; // Await the blob data to make sure hash validation passes.

        Ok(kind)
    }

    #[fbinit::compat_test]
    async fn test_prepare_blob(fb: FacebookInit) -> Result<(), Error> {
        let repo = blobrepo_factory::new_memblob_empty(None)?;
        let blob = roundtrip_blob(fb, &repo, "foo").await?;
        assert_matches!(blob, RemotefilelogBlobKind::Inline(3));
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_prepare_blob_chunked(fb: FacebookInit) -> Result<(), Error> {
        let repo = blobrepo_factory::new_memblob_empty(None)?.dangerous_override(
            |mut config: FilestoreConfig| {
                config.chunk_size = Some(1);
                config
            },
        );

        let blob = roundtrip_blob(fb, &repo, "foo").await?;
        assert_matches!(blob, RemotefilelogBlobKind::Inline(3));
        Ok(())
    }
}
