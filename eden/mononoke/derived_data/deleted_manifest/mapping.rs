/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::BlobstoreGetData;
use blobstore::Loadable;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ChangesetId;
use mononoke_types::MononokeId;

/// Common trait for the root id of all deleted manifest types
pub trait RootDeletedManifestIdCommon:
    BonsaiDerivable
    + std::fmt::Debug
    + Clone
    + Into<BlobstoreBytes>
    + TryFrom<BlobstoreGetData, Error = anyhow::Error>
{
    /// The manifest type
    type Manifest: DeletedManifestCommon<Id = Self::Id>;
    /// The id type (Manifest::Id)
    // Basically just a type alias to Manifest::Id, but Rust makes us be a bit more verbose
    type Id: MononokeId<Value = Self::Manifest> + Loadable<Value = Self::Manifest>;

    /// Create a root DM id
    fn new(id: Self::Id) -> Self;

    /// Get the id of the root deleted manifest node
    fn id(&self) -> &Self::Id;

    /// Create a key for this DM in the given Changeset
    fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String;
}
