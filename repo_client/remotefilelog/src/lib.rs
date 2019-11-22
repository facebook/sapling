/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod redaction;

use std::{
    collections::HashSet,
    io::{Cursor, Write},
};

use blobrepo::{file_history::get_file_history, BlobRepo};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Result};
use futures::{Future, IntoFuture, Stream};
use futures_ext::{select_all, BoxFuture, FutureExt};
use mercurial_types::{
    blobs::File, calculate_hg_node_id, FileBytes, HgFileEnvelopeMut, HgFileHistoryEntry,
    HgFileNodeId, HgParents, MPath, RevFlags,
};
use revisionstore::Metadata;
use thiserror::Error;

use redaction::RedactionFutureExt;

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Corrupt hg filenode returned: {expected} != {actual}")]
    CorruptHgFileNode {
        expected: HgFileNodeId,
        actual: HgFileNodeId,
    },

    #[error("Invalid blob kind returned: {kind:?}")]
    InvalidKind { kind: RemotefilelogBlobKind },
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

/// Remotefilelog blob consists of file content in `node` revision and all the history
/// of the file up to `node`
pub fn create_getfiles_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    path: MPath,
    lfs_threshold: Option<u64>,
    validate_hash: bool,
) -> impl Future<Item = Bytes, Error = Error> {
    let raw_content_bytes = prepare_blob(
        ctx.clone(),
        repo.clone(),
        node,
        lfs_threshold,
        validate_hash,
    )
    .and_then(|RemotefilelogBlob { kind, data }| (Ok(kind).into_future(), data.rescue_redacted()))
    .and_then(move |(kind, (_meta_bytes, file_bytes))| {
        use RemotefilelogBlobKind::*;

        let revlog_flags = match kind {
            Inline(_) => RevFlags::REVIDX_DEFAULT_FLAGS,
            Lfs => RevFlags::REVIDX_EXTSTORED,
        };

        encode_getfiles_file_content(file_bytes, revlog_flags)
    });

    let file_history_bytes = get_file_history(ctx, repo, node, path, None)
        .collect()
        .and_then(serialize_history);

    raw_content_bytes
        .join(file_history_bytes)
        .map(|(mut raw_content, file_history)| {
            raw_content.extend(file_history);
            raw_content
        })
        .and_then(|content| lz4_pyframe::compress(&content))
        .map(|bytes| Bytes::from(bytes))
        .boxify()
}

fn encode_getfiles_file_content(
    raw_content: FileBytes,
    meta_key_flag: RevFlags,
) -> Result<Vec<u8>> {
    let raw_content = raw_content.into_bytes();
    // requires digit counting to know for sure, use reasonable approximation
    let approximate_header_size = 12;
    let mut writer = Cursor::new(Vec::with_capacity(
        approximate_header_size + raw_content.len(),
    ));

    // Write header
    let res = write!(
        writer,
        "v1\n{}{}\n{}{}\0",
        METAKEYSIZE,
        raw_content.len(),
        METAKEYFLAG,
        meta_key_flag,
    );

    res.and_then(|_| writer.write_all(&raw_content))
        .map_err(Error::from)
        .map(|_| writer.into_inner())
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
    prepare_blob(ctx, repo, node, None, validate_hash).map(
        move |RemotefilelogBlob { kind, data }| {
            use RemotefilelogBlobKind::*;

            let weight = match kind {
                Inline(size) => size,
                Lfs => unreachable!(), // lfs_threshold = None implies no LFS blobs.
            };

            let fut = data
                .rescue_redacted()
                .map(move |(mut meta_bytes, file_bytes)| {
                    // TODO (T30456231): Avoid this copy
                    meta_bytes.extend_from_slice(&file_bytes.as_bytes());
                    (node, meta_bytes)
                });

            (weight, fut)
        },
    )
}

/// Create a blob for getpack v2. See v1 above for general details. This also returns Metadata,
/// which is present in the v2 version of the protocol.
pub fn create_getpack_v2_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    lfs_threshold: Option<u64>,
    validate_hash: bool,
) -> impl Future<
    Item = (
        u64,
        impl Future<Item = (HgFileNodeId, Bytes, Metadata), Error = Error>,
    ),
    Error = Error,
> {
    prepare_blob(ctx, repo, node, lfs_threshold, validate_hash).map(
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

            let fut = data
                .rescue_redacted()
                .map(move |(mut meta_bytes, file_bytes)| {
                    // TODO (T30456231): Avoid this copy
                    meta_bytes.extend_from_slice(&file_bytes.as_bytes());
                    (node, meta_bytes, metadata)
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
    prepare_blob(ctx, repo, node, None, validate_hash)
        .and_then(|RemotefilelogBlob { kind, data }| {
            use RemotefilelogBlobKind::*;

            match kind {
                Inline(_) => data.left_future(),
                kind @ _ => Err(ErrorKind::InvalidKind { kind }.into())
                    .into_future()
                    .right_future(),
            }
        })
        .map(|(mut meta_bytes, file_bytes)| {
            // TODO (T30456231): Avoid this copy
            meta_bytes.extend_from_slice(&file_bytes.as_bytes());
            meta_bytes
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
    select_all(
        filenodes.into_iter().map(|filenode| {
            get_file_history(ctx.clone(), repo.clone(), filenode, path.clone(), None)
        }),
    )
    .filter({
        let mut used_filenodes = HashSet::new();
        move |entry| used_filenodes.insert(entry.filenode().clone())
    })
}

/// Convert file history into bytes as expected in Mercurial's loose file format.
fn serialize_history(history: Vec<HgFileHistoryEntry>) -> Result<Vec<u8>> {
    let approximate_history_entry_size = 81;
    let mut writer = Cursor::new(Vec::<u8>::with_capacity(
        history.len() * approximate_history_entry_size,
    ));

    for entry in history {
        entry.write_to_loose_file(&mut writer)?;
    }

    Ok(writer.into_inner())
}

fn prepare_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    lfs_threshold: Option<u64>,
    validate_hash: bool,
) -> impl Future<Item = RemotefilelogBlob, Error = Error> {
    repo.get_file_envelope(ctx.clone(), node).map({
        cloned!(repo);
        move |envelope| {
            let file_size = envelope.content_size();

            let inline_file = match lfs_threshold {
                Some(lfs_threshold) => (file_size <= lfs_threshold),
                None => true,
            };

            // NOTE: It'd be nice if we could hoist up redaction checks to this point. Doing so
            // would let us return a different kind based on whether the content is redacted or
            // not, and therefore would make it more obvious which methods do redaction or not
            // (based on their signature).

            if inline_file {
                let content_fut = repo
                    .get_file_content_by_content_id(ctx, envelope.content_id())
                    .concat2();

                let blob_fut = if validate_hash {
                    content_fut
                        .and_then(move |file_bytes| {
                            let HgFileEnvelopeMut {
                                p1, p2, metadata, ..
                            } = envelope.into_mut();

                            let mut validation_bytes =
                                Bytes::with_capacity(metadata.len() + file_bytes.as_bytes().len());
                            validation_bytes.extend_from_slice(&metadata);
                            validation_bytes.extend_from_slice(file_bytes.as_bytes());

                            let p1 = p1.map(|p| p.into_nodehash());
                            let p2 = p2.map(|p| p.into_nodehash());
                            let actual = HgFileNodeId::new(calculate_hg_node_id(
                                &validation_bytes,
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
                let blob_fut = (
                    repo.get_file_sha256(ctx, envelope.content_id()),
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
