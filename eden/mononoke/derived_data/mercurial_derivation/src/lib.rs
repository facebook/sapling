/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod acl_overlay_manifest;
mod augmented_manifest_v2;
mod augmented_pipeline;
pub mod derive_hg_augmented_manifest;
pub mod derive_hg_changeset;
pub mod derive_hg_manifest;
// Rich, sharded-backed adapter types for the no-Hg Bonsai-direct augmented
// path (aug-manifest-v2 Option 1).
mod indexed_augmented_manifest;
mod mapping;
pub mod pipeline;

pub use augmented_manifest_v2::RootHgAugmentedManifestV2Id;
pub use derive_hg_changeset::DeriveHgChangeset;
pub use derive_hg_changeset::derive_hg_augmented_manifest_at_creation;
pub use derive_hg_changeset::derive_hg_changeset;
pub use derive_hg_changeset::get_manifest_entry_from_bonsai;
pub use derive_hg_changeset::get_manifest_from_bonsai;
pub use derive_hg_manifest::derive_hg_manifest;
pub use mapping::MappedHgChangesetId;
pub use mapping::RootHgAugmentedManifestId;
