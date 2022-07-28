/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use context::CoreContext;
use futures::stream::BoxStream;
use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::blob::BlobstoreValue;
use crate::ChangesetId;
use crate::MPathElement;
use crate::MononokeId;

/// This trait has common behaviour that should be shared among all versions
/// of deleted manifest, and should be used to generalize usage of them.
#[async_trait::async_trait]
pub trait DeletedManifestCommon:
    BlobstoreValue<Key = Self::Id> + Debug + Clone + Send + Sync + 'static
{
    type Id: MononokeId<Value = Self> + Loadable<Value = Self>;

    /// Create a new deleted manifest by copying subentries from `current` and then
    /// adding the subentries from `subentries_to_add` (where `None` means "remove")
    async fn copy_and_update_subentries(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        current: Option<Self>,
        linknode: Option<ChangesetId>,
        subentries_to_add: BTreeMap<MPathElement, Option<Self::Id>>,
    ) -> Result<Self>;

    /// Lookup a specific subentry on this manifest.
    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        basename: &MPathElement,
    ) -> Result<Option<Self::Id>>;

    /// List all subentries on this manifest. Use with care, some manifests can
    /// have hundreds of thousands of subentries.
    fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, Self::Id)>>;

    /// Returns whether this node has no subentries.
    fn is_empty(&self) -> bool;

    /// Whether the file/directory represented by this manifest node currently exists
    /// in the changeset.
    fn is_deleted(&self) -> bool {
        self.linknode().is_some()
    }

    /// Calculate id of manifest object.
    fn id(&self) -> Self::Id;

    /// Last changeset where the this node was deleted
    fn linknode(&self) -> Option<&ChangesetId>;
}
