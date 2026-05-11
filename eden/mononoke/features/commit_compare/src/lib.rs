/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

pub mod commit_compare_path;
pub mod conversions;
pub mod identity;
pub mod operations;

use acl_regions::AclRegionsRef;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_svnrev_mapping::BonsaiSvnrevMappingRef;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphRef;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use metaconfig_types::RepoConfigRef;
use mononoke_api::RepoWithBubble;
use mutable_renames::MutableRenamesArc;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use repo_permission_checker::RepoPermissionCheckerRef;
use restricted_paths::RestrictedPathsArc;

/// The repo-attribute bounds that `commit_compare` requires.
///
/// This is the union of bounds on the mononoke_api methods that
/// `commit_compare` calls, so any facet container that satisfies these
/// can be plugged in (e.g. the diff service's slim repo container).
pub trait Repo = RepoPermissionCheckerRef
    + AclRegionsRef
    + RepoIdentityRef
    + RepoConfigRef
    + RestrictedPathsArc
    + RepoBlobstoreRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + RepoDerivedDataArc
    + RepoEphemeralStoreRef
    + RepoWithBubble
    + CommitGraphRef
    + CommitGraphArc
    + BonsaiHgMappingRef
    + BonsaiGitMappingRef
    + BonsaiGlobalrevMappingRef
    + BonsaiSvnrevMappingRef
    + MutableRenamesArc
    + Clone
    + Send
    + Sync
    + 'static;
