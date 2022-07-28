/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use acl_regions::AclRegions;
use blobrepo::AsBlobRepo;
use blobrepo::BlobRepo;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use ephemeral_blobstore::RepoEphemeralStore;
use mercurial_mutation::HgMutationStore;
use metaconfig_types::RepoConfig;
use mutable_counters::MutableCounters;
use mutable_renames::MutableRenames;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repo_lock::RepoLock;
use repo_permission_checker::RepoPermissionChecker;
use repo_sparse_profiles::RepoSparseProfiles;
use segmented_changelog_types::SegmentedChangelog;
use skiplist::SkiplistIndex;
use streaming_clone::StreamingClone;

// Eventually everything inside Repo should really be here
// The fields of BlobRepo that are not used in e.g. LFS server should also be moved here
// Each binary will then be able to only build what they use of the "repo attributes".
#[facet::container]
#[derive(Clone)]
pub struct InnerRepo {
    #[delegate(
        RepoBlobstore,
        RepoBookmarkAttrs,
        RepoDerivedData,
        RepoIdentity,
        dyn BonsaiGitMapping,
        dyn BonsaiGlobalrevMapping,
        dyn BonsaiHgMapping,
        dyn BookmarkUpdateLog,
        dyn Bookmarks,
        dyn ChangesetFetcher,
        dyn Changesets,
        dyn Phases,
        dyn PushrebaseMutationMapping,
        dyn HgMutationStore,
        dyn MutableCounters,
        dyn RepoPermissionChecker,
        dyn RepoLock,
    )]
    pub blob_repo: BlobRepo,

    #[facet]
    pub repo_config: RepoConfig,

    #[facet]
    pub skiplist_index: SkiplistIndex,

    #[facet]
    pub segmented_changelog: dyn SegmentedChangelog,

    #[facet]
    pub ephemeral_store: RepoEphemeralStore,

    #[facet]
    pub mutable_renames: MutableRenames,

    #[facet]
    pub repo_cross_repo: RepoCrossRepo,

    #[facet]
    pub acl_regions: dyn AclRegions,

    #[facet]
    pub sparse_profiles: RepoSparseProfiles,

    #[facet]
    pub streaming_clone: StreamingClone,
}

impl AsBlobRepo for InnerRepo {
    fn as_blob_repo(&self) -> &BlobRepo {
        &self.blob_repo
    }
}
