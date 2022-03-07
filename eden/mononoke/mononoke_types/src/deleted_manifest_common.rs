/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::Loadable;

use crate::{blob::BlobstoreValue, ChangesetId, MPathElement, MononokeId};

/// This trait has common behaviour that should be shared among all versions
/// of deleted manifest, and should be used to generalize usage of them.
pub trait DeletedManifestCommon: BlobstoreValue<Key = Self::Id> + Clone + Send {
    type Id: MononokeId<Value = Self> + Loadable<Value = Self>;

    /// Create a new deleted manifest by copying subentries from `current` and then
    /// adding the subentries from `subentries_to_add` (where `None` means "remove")
    fn copy_and_update_subentries(
        current: Option<Self>,
        linknode: Option<ChangesetId>,
        subentries_to_add: impl IntoIterator<Item = (MPathElement, Option<Self::Id>)>,
    ) -> Self;

    /// Lookup a specific subentry on this manifest.
    fn lookup(&self, basename: &MPathElement) -> Option<&Self::Id>;

    /// List all subentries on this manifest. Use with care, some manifests can
    /// have hundreds of thousands of subentries.
    fn into_subentries(self) -> Box<dyn Iterator<Item = (MPathElement, Self::Id)>>;

    /// Returns whether this node has no subentries.
    fn is_empty(&self) -> bool;

    /// Whether the file/directory represented by this manifest node currently exists
    /// in the changeset.
    fn is_deleted(&self) -> bool;

    /// Calculate id of manifest object.
    fn id(&self) -> Self::Id;
}
