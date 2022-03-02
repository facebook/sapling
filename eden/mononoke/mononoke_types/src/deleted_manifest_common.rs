/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::Loadable;
use sorted_vector_map::SortedVectorMap;

use crate::{blob::BlobstoreValue, ChangesetId, MPathElement, MononokeId};

pub trait DeletedManifestCommon: BlobstoreValue<Key = Self::Id> + Send {
    type Id: MononokeId<Value = Self> + Loadable<Value = Self>;

    fn is_deleted(&self) -> bool;

    fn into_subentries(self) -> Box<dyn Iterator<Item = (MPathElement, Self::Id)>>;

    fn new(
        linknode: Option<ChangesetId>,
        subentries: SortedVectorMap<MPathElement, Self::Id>,
    ) -> Self;

    fn id(&self) -> Self::Id;
}
