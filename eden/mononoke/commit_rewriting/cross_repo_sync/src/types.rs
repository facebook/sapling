/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Deref;
use std::ops::DerefMut;

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingArc;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Bookmarks;
use bookmarks::BookmarksArc;
use bookmarks::BookmarksRef;
use changesets::Changesets;
use changesets::ChangesetsRef;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use filestore::FilestoreConfig;
use filestore::FilestoreConfigRef;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoConfigRef;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersArc;
use phases::Phases;
use phases::PhasesRef;
use pushrebase::PushrebaseError;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use pushrebase_mutation_mapping::PushrebaseMutationMappingRef;
use ref_cast::RefCast;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_bookmark_attrs::RepoBookmarkAttrsRef;
use repo_cross_repo::RepoCrossRepo;
use repo_cross_repo::RepoCrossRepoRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use static_assertions::assert_impl_all;
use thiserror::Error;

macro_rules! generic_newtype_with_obvious_impls {
    ($name: ident) => {
        #[derive(RefCast)]
        #[repr(transparent)]
        pub struct $name<T>(pub T);

        impl<T: Debug> Debug for $name<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl<T: Display> Display for $name<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl<T: PartialEq> PartialEq for $name<T> {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl<T: Eq> Eq for $name<T> {}

        impl<T: Copy> Copy for $name<T> {}

        impl<T: Clone> Clone for $name<T> {
            fn clone(&self) -> Self {
                Self(self.0.clone())
            }
        }

        impl<T: Clone> $name<&T> {
            pub fn cloned(&self) -> $name<T> {
                $name(self.0.clone())
            }
        }

        impl<T: Hash> Hash for $name<T> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.hash(state)
            }
        }

        impl<T> Deref for $name<T> {
            type Target = T;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<T> DerefMut for $name<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl<T> $name<T> {
            pub const fn as_ref(&self) -> $name<&T> {
                $name(&self.0)
            }

            pub fn as_mut(&mut self) -> $name<&mut T> {
                $name(&mut self.0)
            }
        }
    };
}

generic_newtype_with_obvious_impls! { Large }
generic_newtype_with_obvious_impls! { Small }
generic_newtype_with_obvious_impls! { Source }
generic_newtype_with_obvious_impls! { Target }

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Pushrebase of synced commit failed - check config for overlaps: {0:?}")]
    PushrebaseFailure(PushrebaseError),
    #[error("Remapped commit {0} expected in target repo, but not present")]
    MissingRemappedCommit(ChangesetId),
    #[error("Could not find a commit in the target repo with the same working copy as {0}")]
    SameWcSearchFail(ChangesetId),
    #[error("Parent commit {0} hasn't been remapped")]
    ParentNotRemapped(ChangesetId),
    #[error("Parent commit {0} is not a sync candidate")]
    ParentNotSyncCandidate(ChangesetId),
    #[error("Cannot choose working copy equivalent for {0}")]
    AmbiguousWorkingCopyEquivalent(ChangesetId),
    #[error(
        "expected {expected_version} mapping version to be used to remap {cs_id}, but actually {actual_version} mapping version was used"
    )]
    UnexpectedVersion {
        expected_version: CommitSyncConfigVersion,
        actual_version: CommitSyncConfigVersion,
        cs_id: ChangesetId,
    },
    #[error("X-repo sync is temporarily disabled, contact source control oncall")]
    XRepoSyncDisabled,
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum PushrebaseRewriteDates {
    Yes,
    No,
}

pub trait Repo = BookmarksArc
    + BookmarksRef
    + BookmarkUpdateLogArc
    + BookmarkUpdateLogRef
    + RepoBlobstoreArc
    + BonsaiHgMappingRef
    + BonsaiGlobalrevMappingArc
    + RepoCrossRepoRef
    + PushrebaseMutationMappingRef
    + RepoBookmarkAttrsRef
    + BonsaiGitMappingRef
    + BonsaiGitMappingArc
    + FilestoreConfigRef
    + ChangesetsRef
    + RepoIdentityRef
    + MutableCountersArc
    + PhasesRef
    + RepoBlobstoreRef
    + RepoConfigRef
    + RepoDerivedDataRef
    + CommitGraphRef
    + Send
    + Sync
    + Clone
    + 'static;

/// Simplest repo that implements cross_repo_sync::Repo trait
#[facet::container]
#[derive(Clone)]
pub struct ConcreteRepo {
    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pushrebase_mutation_mapping: dyn PushrebaseMutationMapping,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    changesets: dyn Changesets,

    #[facet]
    id: RepoIdentity,

    #[facet]
    phases: dyn Phases,

    #[facet]
    repo_cross_repo: RepoCrossRepo,

    #[facet]
    repo_bookmark_attrs: RepoBookmarkAttrs,

    #[facet]
    config: RepoConfig,

    #[facet]
    derived_data: RepoDerivedData,

    #[facet]
    blobstore: RepoBlobstore,

    #[facet]
    mutable_counters: dyn MutableCounters,

    #[facet]
    commit_graph: CommitGraph,
}

assert_impl_all!(ConcreteRepo: Repo);

/// Syncing commits from a small Mononoke repo with submodule file changes to a
/// large repo requires the small repo submodule dependencies to be available.
///
/// However, LargeToSmall sync and some SmallToLarge operations don't require
/// loading these repos, in which case this value will be set to `None`.
/// When rewriting commits from small to large (i.e. calling `rewrite_commit`),
/// this map has to be available, or the operation will crash otherwise.
#[derive(Clone)]
pub enum SubmoduleDeps<R> {
    ForSync(HashMap<NonRootMPath, R>),
    NotNeeded,
}

impl<R> Default for SubmoduleDeps<R> {
    fn default() -> Self {
        Self::NotNeeded
    }
}
