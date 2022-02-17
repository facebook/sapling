/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::{AsBlobRepo, BlobRepo};
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::{BookmarkUpdateLog, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use mononoke_types::RepositoryId;
use phases::Phases;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use repo_blobstore::RepoBlobstore;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_identity::{RepoIdentity, RepoIdentityRef};

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[delegate(
        RepoBlobstore,
        RepoDerivedData,
        RepoIdentity,
        dyn BonsaiGitMapping,
        dyn BonsaiGlobalrevMapping,
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        dyn BookmarkUpdateLog,
        dyn ChangesetFetcher,
        dyn Changesets,
        dyn Phases,
        dyn PushrebaseMutationMapping,
    )]
    blob_repo: BlobRepo,

    #[facet]
    repo_cross_repo: RepoCrossRepo,
}

impl Repo {
    pub fn repo_id(&self) -> RepositoryId {
        self.repo_identity().id()
    }

    pub fn name(&self) -> &str {
        self.repo_identity().name()
    }
}

impl AsBlobRepo for Repo {
    fn as_blob_repo(&self) -> &BlobRepo {
        &self.blob_repo
    }
}
