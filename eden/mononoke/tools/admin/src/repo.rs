/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::{self, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use ephemeral_blobstore::RepoEphemeralStore;
use phases::Phases;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

/// Repository object for admin commands.
#[facet::container]
#[derive(Clone)]
pub struct AdminRepo {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pub bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub repo_ephemeral_store: RepoEphemeralStore,

    #[facet]
    pub changeset_fetcher: dyn ChangesetFetcher,

    #[facet]
    pub changesets: dyn Changesets,

    #[facet]
    pub phases: dyn Phases,
}
