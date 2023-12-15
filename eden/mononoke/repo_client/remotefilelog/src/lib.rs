/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fmt;

use anyhow::Result;
use blobrepo::AsBlobRepo;
use blobrepo_hg::file_history::get_file_history_maybe_incomplete;
use blobstore::Loadable;
use bytes::Bytes;
use bytes::BytesMut;
use cloned::cloned;
use context::CoreContext;
use filestore::FetchKey;
use futures::future;
use futures::future::BoxFuture;
use futures::stream::select_all::select_all;
use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use getbundle_response::SessionLfsParams;
use mercurial_types::blobs::File;
use mercurial_types::calculate_hg_node_id;
use mercurial_types::FileBytes;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgFileEnvelopeMut;
use mercurial_types::HgFileHistoryEntry;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgParents;
use mercurial_types::NonRootMPath;
use mercurial_types::RevFlags;
use redactedblobstore::has_redaction_root_cause;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use revisionstore_types::Metadata;
use thiserror::Error;

#[facet::container]
pub struct Repo(RepoBlobstore);

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
    /// An LFS filenode, together with the actual file size.
    Lfs(u64),
}

struct RemotefilelogBlob {
    kind: RemotefilelogBlobKind,
    /// data is a future of the metadata bytes and file bytes. For LFS blobs, the metadata bytes
    /// will be empty and the file bytes will contain a serialized LFS pointer.
    data: BoxFuture<'static, Result<(Bytes, FileBytes)>>,
}

impl fmt::Debug for RemotefilelogBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RemotefilelogBlob {{ kind: {:?} }}", self.kind)
    }
}

pub struct GetpackBlobInfo {
    pub filesize: u64,
    // weight is equal to file size if it's a non-lfs blobs
    // or it's zero for lfs blobs
    pub weight: u64,
}

fn rescue_redacted(res: Result<(Bytes, FileBytes)>) -> Result<(Bytes, FileBytes)> {
    /// Tombstone string to replace the content of redacted files with
    const REDACTED_CONTENT: &str = "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";
    match res {
        Ok(b) => Ok(b),
        Err(e) => {
            if has_redaction_root_cause(&e) {
                let ret = (Bytes::new(), FileBytes(REDACTED_CONTENT.as_bytes().into()));
                Ok(ret)
            } else {
                Err(e)
            }
        }
    }
}

/// Create a blob for getpack v1. This returns a future that resolves with an estimated weight for
/// this blob (this is NOT trying to be correct, it's just a rough estimate!), and the blob's
/// bytes.
pub async fn create_getpack_v1_blob(
    ctx: &CoreContext,
    repo: &impl RepoLike,
    node: HgFileNodeId,
    validate_hash: bool,
) -> Result<(
    GetpackBlobInfo,
    impl Future<Output = Result<(HgFileNodeId, Bytes)>>,
)> {
    let RemotefilelogBlob { kind, data } = prepare_blob(
        ctx,
        repo,
        node,
        SessionLfsParams { threshold: None },
        validate_hash,
    )
    .await?;
    use RemotefilelogBlobKind::*;

    let getpack_blob_data = match kind {
        Inline(size) => GetpackBlobInfo {
            filesize: size,
            weight: size,
        },
        Lfs(_) => unreachable!(), // lfs_threshold = None implies no LFS blobs.
    };

    let fut = data
        .map(rescue_redacted)
        .map_ok(move |(meta_bytes, file_bytes)| {
            // TODO (T30456231): Avoid this copy
            let mut buff = BytesMut::with_capacity(meta_bytes.len() + file_bytes.as_bytes().len());
            buff.extend_from_slice(&meta_bytes);
            buff.extend_from_slice(file_bytes.as_bytes());
            (node, buff.freeze())
        });

    Ok((getpack_blob_data, fut))
}

/// Create a blob for getpack v2. See v1 above for general details. This also returns Metadata,
/// which is present in the v2 version of the protocol.
pub async fn create_getpack_v2_blob(
    ctx: &CoreContext,
    repo: &impl RepoLike,
    node: HgFileNodeId,
    lfs_params: SessionLfsParams,
    validate_hash: bool,
) -> Result<(
    GetpackBlobInfo,
    impl Future<Output = Result<(HgFileNodeId, Bytes, Metadata)>>,
)> {
    let RemotefilelogBlob { kind, data } =
        prepare_blob(ctx, repo, node, lfs_params, validate_hash).await?;
    use RemotefilelogBlobKind::*;

    let (weight, metadata) = match kind {
        Inline(size) => {
            let getpack_blob_data = GetpackBlobInfo {
                filesize: size,
                weight: size,
            };
            (
                getpack_blob_data,
                Metadata {
                    size: None,
                    flags: None,
                },
            )
        }
        Lfs(size) => {
            let getpack_blob_data = GetpackBlobInfo {
                filesize: size,
                weight: 0,
            };
            let flags = Some(RevFlags::REVIDX_EXTSTORED.into());
            (getpack_blob_data, Metadata { size: None, flags })
        }
    };

    let fut = data
        .map(rescue_redacted)
        .map_ok(move |(meta_bytes, file_bytes)| {
            // TODO (T30456231): Avoid this copy
            let mut buff = BytesMut::with_capacity(meta_bytes.len() + file_bytes.as_bytes().len());
            buff.extend_from_slice(&meta_bytes);
            buff.extend_from_slice(file_bytes.as_bytes());
            (node, buff.freeze(), metadata)
        });

    Ok((weight, fut))
}

/// Retrieve the raw contents of a filenode. This does not substitute redacted content
/// (it'll just let the redacted error fall through).
pub async fn create_raw_filenode_blob(
    ctx: &CoreContext,
    repo: &impl RepoLike,
    node: HgFileNodeId,
    validate_hash: bool,
) -> Result<Bytes> {
    let RemotefilelogBlob { kind, data } = prepare_blob(
        ctx,
        repo,
        node,
        SessionLfsParams { threshold: None },
        validate_hash,
    )
    .await?;

    let (meta_bytes, file_bytes) = match kind {
        RemotefilelogBlobKind::Inline(_) => data.await?,
        kind => return Err(ErrorKind::InvalidKind { kind }.into()),
    };

    // TODO (T30456231): Avoid this copy
    let mut buff = BytesMut::with_capacity(meta_bytes.len() + file_bytes.as_bytes().len());
    buff.extend_from_slice(&meta_bytes);
    buff.extend_from_slice(file_bytes.as_bytes());
    Ok(buff.freeze())
}

/// Get ancestors of all filenodes
/// Current implementation might be inefficient because it might re-fetch the same filenode a few
/// times
pub fn get_unordered_file_history_for_multiple_nodes(
    ctx: &CoreContext,
    repo: &(impl RepoLike + AsBlobRepo),
    filenodes: HashSet<HgFileNodeId>,
    path: &NonRootMPath,
    allow_short_getpack_history: bool,
) -> impl Stream<Item = Result<HgFileHistoryEntry>> {
    let limit = if allow_short_getpack_history {
        const REMOTEFILELOG_FILE_HISTORY_LIMIT: u64 = 1000;
        Some(REMOTEFILELOG_FILE_HISTORY_LIMIT)
    } else {
        None
    };
    select_all(filenodes.into_iter().map(|filenode| {
        get_file_history_maybe_incomplete(
            ctx.clone(),
            repo.as_blob_repo().clone(),
            filenode,
            path.clone(),
            limit,
        )
        .boxed()
    }))
    .try_filter({
        let mut used_filenodes = HashSet::new();
        move |entry| future::ready(used_filenodes.insert(entry.filenode().clone()))
    })
}

async fn prepare_blob(
    ctx: &CoreContext,
    repo: &impl RepoLike,
    node: HgFileNodeId,
    lfs_params: SessionLfsParams,
    validate_hash: bool,
) -> Result<RemotefilelogBlob> {
    let envelope = node.load(ctx, repo.repo_blobstore()).await?;

    let inline_file = match lfs_params.threshold {
        Some(lfs_threshold) => envelope.content_size() <= lfs_threshold,
        None => true,
    };

    // NOTE: It'd be nice if we could hoist up redaction checks to this point. Doing so
    // would let us return a different kind based on whether the content is redacted or
    // not, and therefore would make it more obvious which methods do redaction or not
    // (based on their signature).
    if inline_file {
        Ok(prepare_blob_inline_file(
            envelope,
            ctx,
            repo,
            node,
            validate_hash,
        ))
    } else {
        Ok(prepare_blob_lfs_file(envelope, ctx, repo))
    }
}

fn prepare_blob_inline_file(
    envelope: HgFileEnvelope,
    ctx: &CoreContext,
    repo: &impl RepoLike,
    node: HgFileNodeId,
    validate_hash: bool,
) -> RemotefilelogBlob {
    cloned!(ctx, repo);
    let kind = RemotefilelogBlobKind::Inline(envelope.content_size());
    let blobstore = repo.repo_blobstore_arc();
    let data = async move {
        let file_bytes =
            FileBytes(filestore::fetch_concat(&blobstore, &ctx, envelope.content_id()).await?);

        let HgFileEnvelopeMut {
            p1, p2, metadata, ..
        } = envelope.into_mut();

        if validate_hash {
            let mut validation_bytes =
                BytesMut::with_capacity(metadata.len() + file_bytes.as_bytes().len());
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
        }

        Ok((metadata, file_bytes))
    }
    .boxed();

    RemotefilelogBlob { kind, data }
}

/// For LFS blobs, we'll create the LFS pointer. Note that there is no hg-style
/// metadata encoded for LFS blobs (it's in the LFS pointer instead).
fn prepare_blob_lfs_file(
    envelope: HgFileEnvelope,
    ctx: &CoreContext,
    repo: &impl RepoLike,
) -> RemotefilelogBlob {
    cloned!(ctx);
    let file_size = envelope.content_size();
    let kind = RemotefilelogBlobKind::Lfs(file_size);
    let blobstore = repo.repo_blobstore_arc();
    let data = async move {
        let key = FetchKey::from(envelope.content_id());
        let oid = filestore::get_metadata(&blobstore, &ctx, &key)
            .await?
            .ok_or(ErrorKind::MissingContent(key))?
            .sha256;
        let copy_from = File::extract_copied_from(envelope.metadata())?;
        let bytes = File::generate_lfs_file(oid, file_size, copy_from)?;
        Ok((Bytes::new(), FileBytes(bytes)))
    }
    .boxed();

    RemotefilelogBlob { kind, data }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use assert_matches::assert_matches;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use manifest::Entry;
    use manifest::Manifest;
    use mercurial_derivation::DeriveHgChangeset;
    use metaconfig_types::FilestoreParams;
    use mononoke_types::MPathElement;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;

    async fn roundtrip_blob(
        fb: FacebookInit,
        repo: &BasicTestRepo,
        content: &str,
        threshold: Option<u64>,
    ) -> Result<RemotefilelogBlobKind> {
        let filename = "f1";

        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        let bcs = CreateCommitContext::new_root(ctx, repo)
            .add_file(filename, content)
            .commit()
            .await?;

        let hg_manifest = repo
            .derive_hg_changeset(ctx, bcs)
            .await?
            .load(ctx, repo.repo_blobstore())
            .await?
            .manifestid()
            .load(ctx, repo.repo_blobstore())
            .await?;

        let entry = hg_manifest
            .lookup(&MPathElement::new(filename.as_bytes().to_vec())?)
            .ok_or_else(|| Error::msg("file is missing"))?;

        let filenode = match entry {
            Entry::Leaf((_, filenode)) => filenode,
            _ => {
                return Err(Error::msg("file is not a leaf"));
            }
        };

        let blob = prepare_blob(ctx, repo, filenode, SessionLfsParams { threshold }, true).await?;

        let RemotefilelogBlob { kind, data } = blob;
        data.await?; // Await the blob data to make sure hash validation passes.

        Ok(kind)
    }

    #[fbinit::test]
    async fn test_prepare_blob(fb: FacebookInit) -> Result<()> {
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
        let blob = roundtrip_blob(fb, &repo, "foo", Some(3)).await?;
        assert_matches!(blob, RemotefilelogBlobKind::Inline(3));
        Ok(())
    }

    #[fbinit::test]
    async fn test_prepare_blob_chunked(fb: FacebookInit) -> Result<()> {
        let repo: BasicTestRepo = TestRepoFactory::new(fb)?
            .with_config_override(|config| {
                config.filestore = Some(FilestoreParams {
                    chunk_size: 1,
                    concurrency: 1,
                })
            })
            .build()
            .await?;

        let blob = roundtrip_blob(fb, &repo, "foo", None).await?;
        assert_matches!(blob, RemotefilelogBlobKind::Inline(3));
        Ok(())
    }

    #[fbinit::test]
    async fn test_prepare_blob_lfs(fb: FacebookInit) -> Result<()> {
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
        let blob = roundtrip_blob(fb, &repo, "foo", Some(2)).await?;
        assert_matches!(blob, RemotefilelogBlobKind::Lfs(3));
        Ok(())
    }
}
