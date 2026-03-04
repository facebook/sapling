/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use crate::Blob;
use crate::BlobstoreValue;
use crate::MPathElement;
use crate::ThriftConvert;
use crate::blob::AclManifestBlob;
use crate::blob::AclManifestEntryBlobBlob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::AclManifestContext;
use crate::typed_hash::AclManifestEntryBlobContext;
use crate::typed_hash::AclManifestEntryBlobId;
use crate::typed_hash::AclManifestId;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeAclManifestContext;
pub use crate::typed_hash::ShardedMapV2NodeAclManifestId;

/// Whether a directory itself is restricted
#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::acl_manifest::AclManifestDirectoryRestriction)]
pub enum AclManifestDirectoryRestriction {
    #[thrift(thrift::acl_manifest::AclManifestUnrestricted)]
    Unrestricted,
    Restricted(AclManifestRestriction),
}

/// A restriction root — points to a separately stored ACL entry blob
#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::acl_manifest::AclManifestRestriction)]
pub struct AclManifestRestriction {
    pub entry_blob_id: AclManifestEntryBlobId,
}

/// Entry in the parent's ShardedMapV2 — directory only (no File variant).
/// Represents a child that is either a restriction root or a waypoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AclManifestDirectoryEntry {
    /// ID of the child's AclManifest node
    pub id: AclManifestId,
    /// Whether this directory is a restriction root
    pub is_restricted: bool,
    /// Whether any descendant of this directory is a restriction root
    pub has_restricted_descendants: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AclManifestEntry {
    AclFile(AclManifestRestriction),
    Directory(AclManifestDirectoryEntry),
}

impl ThriftConvert for AclManifestEntry {
    const NAME: &'static str = "AclManifestEntry";
    type Thrift = thrift::acl_manifest::AclManifestEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        match t {
            thrift::acl_manifest::AclManifestEntry::acl_file(restriction) => Ok(
                AclManifestEntry::AclFile(AclManifestRestriction::from_thrift(restriction)?),
            ),
            thrift::acl_manifest::AclManifestEntry::directory(dir) => {
                Ok(AclManifestEntry::Directory(AclManifestDirectoryEntry {
                    id: AclManifestId::from_thrift(dir.id)?,
                    is_restricted: dir.is_restricted,
                    has_restricted_descendants: dir.has_restricted_descendants,
                }))
            }
            thrift::acl_manifest::AclManifestEntry::UnknownField(x) => {
                anyhow::bail!("Unknown AclManifestEntry variant: {}", x)
            }
        }
    }

    fn into_thrift(self) -> Self::Thrift {
        match self {
            AclManifestEntry::AclFile(restriction) => {
                thrift::acl_manifest::AclManifestEntry::acl_file(restriction.into_thrift())
            }
            AclManifestEntry::Directory(dir) => thrift::acl_manifest::AclManifestEntry::directory(
                thrift::acl_manifest::AclManifestDirectoryEntry {
                    id: dir.id.into_thrift(),
                    is_restricted: dir.is_restricted,
                    has_restricted_descendants: dir.has_restricted_descendants,
                },
            ),
        }
    }
}

/// A node in the sparse ACL manifest tree.
/// Only exists for restriction roots and their ancestors.
#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::acl_manifest::AclManifest)]
pub struct AclManifest {
    /// Whether THIS directory is a restriction root
    pub restriction: AclManifestDirectoryRestriction,
    /// Children that are restriction roots or waypoints
    pub subentries: ShardedMapV2Node<AclManifestEntry>,
}

/// Rollup data for ShardedMapV2 pruning
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct AclManifestRollup {
    /// True if any Directory entry exists in this subtree of the sharded map.
    /// AclFile (leaf) entries are excluded — only directories representing
    /// restriction roots or waypoints count. This enables efficient computation
    /// of `has_restricted_descendants` without loading all entries.
    pub has_restricted: bool,
}

impl ThriftConvert for AclManifestRollup {
    const NAME: &'static str = "AclManifestRollup";
    type Thrift = thrift::acl_manifest::AclManifestRollup;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(AclManifestRollup {
            has_restricted: t.has_restricted,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::acl_manifest::AclManifestRollup {
            has_restricted: self.has_restricted,
        }
    }
}

impl ShardedMapV2Value for AclManifestEntry {
    type NodeId = ShardedMapV2NodeAclManifestId;
    type Context = ShardedMapV2NodeAclManifestContext;
    type RollupData = AclManifestRollup;

    const WEIGHT_LIMIT: usize = 500;
}

impl Rollup<AclManifestEntry> for AclManifestRollup {
    fn rollup(value: Option<&AclManifestEntry>, child_rollup_data: Vec<Self>) -> Self {
        // Only Directory entries count — AclFile entries don't indicate
        // restricted descendants, just that this directory itself is restricted.
        let is_directory = value.is_some_and(|e| matches!(e, AclManifestEntry::Directory(_)));
        Self {
            has_restricted: is_directory || child_rollup_data.iter().any(|r| r.has_restricted),
        }
    }
}

impl AclManifest {
    pub fn empty() -> Self {
        Self {
            restriction: AclManifestDirectoryRestriction::Unrestricted,
            subentries: ShardedMapV2Node::default(),
        }
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl KeyedBlobstore,
        name: &MPathElement,
    ) -> Result<Option<AclManifestEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl KeyedBlobstore,
    ) -> BoxStream<'a, Result<(MPathElement, AclManifestEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }
}

impl BlobstoreValue for AclManifest {
    type Key = AclManifestId;

    fn into_blob(self) -> AclManifestBlob {
        let data = self.into_bytes();
        let id = AclManifestContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

/// Individually-stored ACL metadata for a restriction root.
/// Content-addressed: identical .slacl files share the same blob.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AclManifestEntryBlob {
    /// REPO_REGION ACL protecting this directory
    pub repo_region_acl: String,
    /// AMP group to direct users to for access requests
    pub permission_request_group: Option<String>,
}

impl ThriftConvert for AclManifestEntryBlob {
    const NAME: &'static str = "AclManifestEntryBlob";
    type Thrift = thrift::acl_manifest::AclManifestEntryBlob;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(AclManifestEntryBlob {
            repo_region_acl: t.repo_region_acl,
            permission_request_group: t.permission_request_group,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::acl_manifest::AclManifestEntryBlob {
            repo_region_acl: self.repo_region_acl,
            permission_request_group: self.permission_request_group,
        }
    }
}

impl BlobstoreValue for AclManifestEntryBlob {
    type Key = AclManifestEntryBlobId;

    fn into_blob(self) -> AclManifestEntryBlobBlob {
        let data = self.into_bytes();
        let id = AclManifestEntryBlobContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
