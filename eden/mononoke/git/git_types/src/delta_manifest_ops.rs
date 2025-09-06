/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::Iterator;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use context::CoreContext;
use futures::stream::BoxStream;
use gix_hash::ObjectId;
use gix_object::Kind;
use metaconfig_types::GitDeltaManifestVersion;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::path::MPath;
use repo_derived_data::RepoDerivedData;

use crate::RootGitDeltaManifestV2Id;
use crate::RootGitDeltaManifestV3Id;
use crate::thrift;

/// Fetches GitDeltaManifest for a given changeset with the given version.
/// Derives the GitDeltaManifest if not present.
pub async fn fetch_git_delta_manifest(
    ctx: &CoreContext,
    derived_data: &RepoDerivedData,
    blobstore: &impl Blobstore,
    git_delta_manifest_version: GitDeltaManifestVersion,
    cs_id: ChangesetId,
) -> Result<Box<dyn GitDeltaManifestOps + Send + Sync>> {
    match git_delta_manifest_version {
        GitDeltaManifestVersion::V2 => {
            let root_mf_id = derived_data
                .derive::<RootGitDeltaManifestV2Id>(ctx, cs_id)
                .await
                .with_context(|| {
                    format!(
                        "Error in deriving RootGitDeltaManifestV2Id for changeset {:?}",
                        cs_id
                    )
                })?;

            Ok(Box::new(
                root_mf_id
                    .manifest_id()
                    .load(ctx, blobstore)
                    .await
                    .with_context(|| {
                        format!(
                            "Error in loading GitDeltaManifestV2 from root id {:?}",
                            root_mf_id
                        )
                    })?,
            ))
        }
        GitDeltaManifestVersion::V3 => {
            let root_mf_id = derived_data
                .derive::<RootGitDeltaManifestV3Id>(ctx, cs_id)
                .await
                .with_context(|| {
                    format!(
                        "Error in deriving RootGitDeltaManifestV3Id for changeset {:?}",
                        cs_id
                    )
                })?;

            Ok(Box::new(
                root_mf_id
                    .manifest_id()
                    .load(ctx, blobstore)
                    .await
                    .with_context(|| {
                        format!(
                            "Error in loading GitDeltaManifestV3 from root id {:?}",
                            root_mf_id
                        )
                    })?,
            ))
        }
    }
}

/// Enum representing the types of Git objects that can be present
/// in a GitDeltaManifest
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ObjectKind {
    Blob,
    Tree,
}

impl ObjectKind {
    pub fn to_gix_kind(&self) -> Kind {
        match self {
            ObjectKind::Blob => Kind::Blob,
            ObjectKind::Tree => Kind::Tree,
        }
    }

    pub fn is_tree(&self) -> bool {
        *self == ObjectKind::Tree
    }

    pub fn is_blob(&self) -> bool {
        *self == ObjectKind::Blob
    }
}

impl TryFrom<thrift::ObjectKind> for ObjectKind {
    type Error = anyhow::Error;

    fn try_from(value: thrift::ObjectKind) -> Result<Self, Self::Error> {
        match value {
            thrift::ObjectKind::Blob => Ok(Self::Blob),
            thrift::ObjectKind::Tree => Ok(Self::Tree),
            thrift::ObjectKind(x) => anyhow::bail!("Unsupported object kind: {}", x),
        }
    }
}

impl From<ObjectKind> for thrift::ObjectKind {
    fn from(value: ObjectKind) -> Self {
        match value {
            ObjectKind::Blob => thrift::ObjectKind::Blob,
            ObjectKind::Tree => thrift::ObjectKind::Tree,
        }
    }
}

impl ThriftConvert for ObjectKind {
    const NAME: &'static str = "ObjectKind";
    type Thrift = thrift::ObjectKind;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

/// Trait representing a version of GitDeltaManifest.
pub trait GitDeltaManifestOps {
    /// Returns a stream of the entries of the GitDeltaManifest. There should
    /// be an entry for each object at a path that differs from the corresponding
    /// object at the same path in one of the parents.
    fn into_entries<'a>(
        self: Box<Self>,
        ctx: &'a CoreContext,
        blobstore: &'a Arc<dyn Blobstore>,
    ) -> BoxStream<'a, Result<Box<dyn GitDeltaManifestEntryOps + Send>>>;
}

/// Trait representing a subentry of a GitDeltaManifest.
pub trait GitDeltaManifestEntryOps {
    /// Returns the path of the subentry.
    fn path(&self) -> &MPath;

    /// Returns the size of the full object.
    fn full_object_size(&self) -> u64;

    /// Returns the OID of the full object.
    fn full_object_oid(&self) -> ObjectId;

    /// Returns the kind of the full object.
    fn full_object_kind(&self) -> ObjectKind;

    /// Returns the RichGitSha1 of the full object.
    fn full_object_rich_git_sha1(&self) -> Result<RichGitSha1> {
        let sha1 = GitSha1::from_bytes(self.full_object_oid().as_bytes())?;
        let ty = match self.full_object_kind() {
            ObjectKind::Blob => "blob",
            ObjectKind::Tree => "tree",
        };
        Ok(RichGitSha1::from_sha1(sha1, ty, self.full_object_size()))
    }

    fn into_full_object_inlined_bytes(&mut self) -> Option<Vec<u8>>;

    /// Returns an iterator over the deltas of the subentry.
    fn deltas(&self) -> Box<dyn Iterator<Item = &(dyn ObjectDeltaOps + Sync)> + '_>;
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

    /// Returns the kind of the base object.
    fn base_object_kind(&self) -> ObjectKind;

    /// Returns the size of the base object in bytes.
    fn base_object_size(&self) -> u64;

    /// Returns the instructions bytes of the delta.
    async fn instruction_bytes(
        &self,
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
    ) -> Result<Vec<u8>>;
}
