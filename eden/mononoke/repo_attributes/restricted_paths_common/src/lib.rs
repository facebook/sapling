/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common types and config-based logic for restricted paths.
//!
//! This crate extracts the types, SQL store, cache, and config-based lookup
//! logic from `restricted_paths` so that derived-data crates can depend on it
//! without creating a dependency cycle.

pub mod cache;
pub mod config_based;
pub mod manifest_id_store;

pub use cache::ManifestIdCache;
pub use cache::RestrictedPathsManifestIdCache;
pub use cache::RestrictedPathsManifestIdCacheBuilder;
pub use config_based::ArcRestrictedPathsConfigBased;
pub use config_based::RestrictedPathsConfigBased;
pub use config_based::RestrictedPathsConfigBasedArc;
pub use config_based::RestrictedPathsConfigBasedRef;
pub use manifest_id_store::ArcRestrictedPathsManifestIdStore;
pub use manifest_id_store::ManifestId;
pub use manifest_id_store::ManifestType;
pub use manifest_id_store::RestrictedPathManifestIdEntry;
pub use manifest_id_store::RestrictedPathsManifestIdStore;
pub use manifest_id_store::SqlRestrictedPathsManifestIdStore;
pub use manifest_id_store::SqlRestrictedPathsManifestIdStoreBuilder;
