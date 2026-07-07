/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Shared async request type name definitions.
//!
//! This crate defines the canonical request type names used by the async
//! requests system. It has no thrift or heavy dependencies — just zero-sized
//! marker structs, a trait, and string constants.
//!
//! Both `requests_table` (SQL layer) and `async_requests` (queue/types layer)
//! depend on this crate, allowing type names to be shared without circular
//! dependencies.

/// Trait providing the canonical name for an async request type.
pub trait RequestTypeName {
    const NAME: &'static str;
}

// -- Megarepo request types --

pub struct MegarepoAddSyncTarget;
impl RequestTypeName for MegarepoAddSyncTarget {
    const NAME: &'static str = "megarepo_add_sync_target";
}

pub struct MegarepoAddBranchingSyncTarget;
impl RequestTypeName for MegarepoAddBranchingSyncTarget {
    const NAME: &'static str = "megarepo_add_branching_sync_target";
}

pub struct MegarepoChangeTargetConfig;
impl RequestTypeName for MegarepoChangeTargetConfig {
    const NAME: &'static str = "megarepo_change_target_config";
}

pub struct MegarepoSyncChangeset;
impl RequestTypeName for MegarepoSyncChangeset {
    const NAME: &'static str = "megarepo_sync_changeset";
}

pub struct MegarepoRemergeSource;
impl RequestTypeName for MegarepoRemergeSource {
    const NAME: &'static str = "megarepo_remerge_source";
}

// -- Other request types --

pub struct AsyncPing;
impl RequestTypeName for AsyncPing {
    const NAME: &'static str = "async_ping";
}

pub struct CommitSparseProfileSize;
impl RequestTypeName for CommitSparseProfileSize {
    const NAME: &'static str = "commit_sparse_profile_size_async";
}

pub struct CommitSparseProfileDelta;
impl RequestTypeName for CommitSparseProfileDelta {
    const NAME: &'static str = "commit_sparse_profile_delta_async";
}

// -- Derived data backfill request types --

pub struct DeriveBoundaries;
impl RequestTypeName for DeriveBoundaries {
    const NAME: &'static str = "derive_boundaries";
}

pub struct DeriveSlice;
impl RequestTypeName for DeriveSlice {
    const NAME: &'static str = "derive_slice";
}

pub struct DeriveBackfill;
impl RequestTypeName for DeriveBackfill {
    const NAME: &'static str = "derive_backfill";
}

pub struct DeriveBackfillRepo;
impl RequestTypeName for DeriveBackfillRepo {
    const NAME: &'static str = "derive_backfill_repo";
}

pub struct MarkTypeEnabled;
impl RequestTypeName for MarkTypeEnabled {
    const NAME: &'static str = "mark_type_enabled";
}

// -- Collected constants --

/// All known async request type names.
pub const ALL_REQUEST_TYPE_NAMES: &[&str] = &[
    MegarepoAddSyncTarget::NAME,
    MegarepoAddBranchingSyncTarget::NAME,
    MegarepoChangeTargetConfig::NAME,
    MegarepoSyncChangeset::NAME,
    MegarepoRemergeSource::NAME,
    AsyncPing::NAME,
    CommitSparseProfileSize::NAME,
    CommitSparseProfileDelta::NAME,
    DeriveBoundaries::NAME,
    DeriveSlice::NAME,
    DeriveBackfill::NAME,
    DeriveBackfillRepo::NAME,
    MarkTypeEnabled::NAME,
];

/// Request types that are part of the derived data backfill system.
pub const BACKFILL_REQUEST_TYPES: &[&str] = &[
    DeriveBoundaries::NAME,
    DeriveSlice::NAME,
    DeriveBackfill::NAME,
    DeriveBackfillRepo::NAME,
    MarkTypeEnabled::NAME,
];
