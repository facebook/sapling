/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::Loadable;

use crate::{blob::BlobstoreValue, ChangesetId, MPathElement, MononokeId};

pub trait DeletedManifestCommon: BlobstoreValue<Key = Self::Id> + Clone + Send {
    type Id: MononokeId<Value = Self> + Loadable<Value = Self>;

    fn is_deleted(&self) -> bool;

    fn into_subentries(self) -> Box<dyn Iterator<Item = (MPathElement, Self::Id)>>;

    fn id(&self) -> Self::Id;

    fn lookup(&self, basename: &MPathElement) -> Option<&Self::Id>;

    /// Create a new deleted manifest by copying subentries from `current` and then
    /// adding the subentries from `subentries_to_add` (where `None` means "remove")
    fn copy_and_update_subentries(
        current: Option<Self>,
        linknode: Option<ChangesetId>,
        subentries_to_add: impl IntoIterator<Item = (MPathElement, Option<Self::Id>)>,
    ) -> Self;

    fn is_empty(&self) -> bool;
}
