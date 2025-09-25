/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Restricted Paths.
//!
//! Abstractions to track a repo's restricted paths, along with their ACLs,
//! and to store the manifest ids of these paths from every revision.

mod manifest_id_store;

pub use crate::manifest_id_store::ArcRestrictedPathsManifestIdStore;
pub use crate::manifest_id_store::ManifestType;
pub use crate::manifest_id_store::RestrictedPathManifestIdEntry;
pub use crate::manifest_id_store::RestrictedPathsManifestIdStore;
pub use crate::manifest_id_store::SqlRestrictedPathsManifestIdStore;
pub use crate::manifest_id_store::SqlRestrictedPathsManifestIdStoreBuilder;
