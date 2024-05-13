/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::Iterator;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use bytes::Bytes;
use bytes::BytesMut;
use context::CoreContext;
use futures::Stream;
use futures::TryStreamExt;
use gix_hash::ObjectId;
use metaconfig_types::GitDeltaManifestVersion;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedData;

use crate::delta::DeltaInstructionChunkIdPrefix;
use crate::delta_manifest::GitDeltaManifest;
use crate::delta_manifest::ObjectKind;
use crate::fetch_delta_instructions;
use crate::GitDeltaManifestEntry;
use crate::ObjectDelta;
use crate::RootGitDeltaManifestId;

/// Fetches GitDeltaManifest for a given changeset with the given version.
/// Derives the GitDeltaManifest if not present.
pub async fn fetch_git_delta_manifest(
    ctx: &CoreContext,
    derived_data: &RepoDerivedData,
    blobstore: &impl Blobstore,
    git_delta_manifest_version: GitDeltaManifestVersion,
    cs_id: ChangesetId,
) -> Result<impl GitDeltaManifestOps + Send + Sync> {
    match git_delta_manifest_version {
        GitDeltaManifestVersion::V1 => {
            let root_mf_id = derived_data
                .derive::<RootGitDeltaManifestId>(ctx, cs_id)
                .await
                .with_context(|| {
                    format!(
                        "Error in deriving RootGitDeltaManifestId for changeset {:?}",
                        cs_id
                    )
                })?;

            Ok(root_mf_id
                .manifest_id()
                .load(ctx, blobstore)
                .await
                .with_context(|| {
                    format!(
                        "Error in loading Git Delta Manifest from root id {:?}",
                        root_mf_id
                    )
                })?)
        }
    }
}

/// Trait representing a version of GitDeltaManifest.
pub trait GitDeltaManifestOps {
    /// The type of the subentries of the GitDeltaManifest.
    type GitDeltaManifestEntryType: GitDeltaManifestEntryOps + Send + Sync;

    /// Returns a stream of the subentries of the GitDeltaManifest. There should
    /// be an entry for each object at a path that differs from the corresponding
    /// object at the same path in one of the parents.
    fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> impl Stream<Item = Result<(MPath, Self::GitDeltaManifestEntryType)>> + Send + 'a;
}

impl GitDeltaManifestOps for GitDeltaManifest {
    type GitDeltaManifestEntryType = GitDeltaManifestEntry;

    fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> impl Stream<Item = Result<(MPath, GitDeltaManifestEntry)>> + Send + 'a {
        GitDeltaManifest::into_subentries(self, ctx, blobstore)
    }
}

/// Trait representing a subentry of a GitDeltaManifest.
pub trait GitDeltaManifestEntryOps {
    /// The type of the deltas of the subentry.
    type ObjectDeltaType: ObjectDeltaOps + Clone + Send + Sync;

    /// Returns the size of the full object.
    fn full_object_size(&self) -> u64;

    /// Returns the OID of the full object.
    fn full_object_oid(&self) -> ObjectId;

    /// Returns the kind of the full object.
    fn full_object_kind(&self) -> ObjectKind;

    /// Returns the RichGitSha1 of the full object.
    fn full_object_rich_git_sha1(&self) -> Result<RichGitSha1>;

    /// Returns an iterator over the deltas of the subentry.
    fn deltas(&self) -> impl Iterator<Item = &Self::ObjectDeltaType>;
}

impl GitDeltaManifestEntryOps for GitDeltaManifestEntry {
    type ObjectDeltaType = ObjectDelta;

    fn full_object_size(&self) -> u64 {
        self.full.size
    }

    fn full_object_oid(&self) -> ObjectId {
        self.full.oid
    }

    fn full_object_kind(&self) -> ObjectKind {
        self.full.kind
    }

    fn full_object_rich_git_sha1(&self) -> Result<RichGitSha1> {
        self.full.as_rich_git_sha1()
    }

    fn deltas(&self) -> impl Iterator<Item = &ObjectDelta> {
        self.deltas.iter()
    }
}

/// Trait representing a delta in a GitDeltaManifest.
#[async_trait]
pub trait ObjectDeltaOps {
    /// Returns the uncompressed size of the instructions.
    fn instructions_uncompressed_size(&self) -> u64;

    /// Returns the compressed size of the instructions.
    fn instructions_compressed_size(&self) -> u64;

    /// Returns the OID of the base object.
    fn base_object_oid(&self) -> ObjectId;

    /// Returns the path of the base object.
    fn base_object_path(&self) -> &MPath;

    /// Returns the kind of the base object.
    fn base_object_kind(&self) -> ObjectKind;

    /// Returns the size of the base object in bytes.
    fn base_object_size(&self) -> u64;

    /// Returns the instructions bytes of the delta.
    async fn instruction_bytes(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        cs_id: ChangesetId,
        path: MPath,
    ) -> Result<Bytes>;
}

#[async_trait]
impl ObjectDeltaOps for ObjectDelta {
    fn instructions_uncompressed_size(&self) -> u64 {
        self.instructions_uncompressed_size
    }

    fn instructions_compressed_size(&self) -> u64 {
        self.instructions_compressed_size
    }

    fn base_object_oid(&self) -> ObjectId {
        self.base.oid
    }

    fn base_object_path(&self) -> &MPath {
        &self.base.path
    }

    fn base_object_kind(&self) -> ObjectKind {
        self.base.kind
    }

    fn base_object_size(&self) -> u64 {
        self.base.size
    }

    async fn instruction_bytes(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        cs_id: ChangesetId,
        path: MPath,
    ) -> Result<Bytes> {
        let chunk_id_prefix =
            DeltaInstructionChunkIdPrefix::new(cs_id, path.clone(), self.origin, path);
        let bytes = fetch_delta_instructions(
            ctx,
            blobstore,
            &chunk_id_prefix,
            self.instructions_chunk_count,
        )
        .try_fold(
            BytesMut::with_capacity(self.instructions_compressed_size as usize),
            |mut acc, bytes| async move {
                acc.extend_from_slice(bytes.as_ref());
                anyhow::Ok(acc)
            },
        )
        .await
        .context("Error in fetching delta instruction bytes from byte stream")?
        .freeze();

        Ok(bytes)
    }
}
