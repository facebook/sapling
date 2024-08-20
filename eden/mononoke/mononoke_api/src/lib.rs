/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(error_generic_member_access)]
#![feature(trait_alias)]

use std::sync::Arc;

use anyhow::Error;
pub use bookmarks::BookmarkCategory;
pub use bookmarks::BookmarkKey;
use mononoke_repos::MononokeRepos;
pub use mononoke_types::RepositoryId;
use repo_identity::RepoIdentityRef;

use crate::repo::RepoContextBuilder;

pub mod changeset;
pub mod changeset_path;
pub mod changeset_path_diff;
pub mod errors;
pub mod file;
pub mod path;
pub mod repo;
pub mod sparse_profile;
pub mod specifiers;
pub mod tree;
mod xrepo;

#[cfg(test)]
mod test;

// Re-export types that are useful for clients.
pub use context::CoreContext;
pub use context::LoggingContainer;
pub use context::SessionContainer;

pub use crate::changeset::ChangesetContext;
pub use crate::changeset::ChangesetDiffItem;
pub use crate::changeset::ChangesetFileOrdering;
pub use crate::changeset::ChangesetHistoryOptions;
pub use crate::changeset::ChangesetLinearHistoryOptions;
pub use crate::changeset::Generation;
pub use crate::changeset_path::ChangesetPathContentContext;
pub use crate::changeset_path::ChangesetPathHistoryOptions;
pub use crate::changeset_path::PathEntry;
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
pub use crate::changeset_path_diff::CopyInfo;
pub use crate::changeset_path_diff::FileContentType;
pub use crate::changeset_path_diff::FileGeneratedStatus;
pub use crate::changeset_path_diff::MetadataDiff;
pub use crate::changeset_path_diff::MetadataDiffFileInfo;
pub use crate::changeset_path_diff::MetadataDiffLinesCount;
pub use crate::changeset_path_diff::UnifiedDiff;
pub use crate::changeset_path_diff::UnifiedDiffMode;
pub use crate::errors::MononokeError;
pub use crate::file::headerless_unified_diff;
pub use crate::file::FileContext;
pub use crate::file::FileId;
pub use crate::file::FileMetadata;
pub use crate::file::FileType;
pub use crate::file::HeaderlessUnifiedDiff;
pub use crate::repo::create_changeset::CreateChange;
pub use crate::repo::create_changeset::CreateChangeFile;
pub use crate::repo::create_changeset::CreateChangeFileContents;
pub use crate::repo::create_changeset::CreateChangeGitLfs;
pub use crate::repo::create_changeset::CreateCopyInfo;
pub use crate::repo::create_changeset::CreateInfo;
pub use crate::repo::land_stack::PushrebaseOutcome;
pub use crate::repo::update_submodule_expansion::SubmoduleExpansionUpdate;
pub use crate::repo::update_submodule_expansion::SubmoduleExpansionUpdateCommitInfo;
pub use crate::repo::BookmarkFreshness;
pub use crate::repo::BookmarkInfo;
pub use crate::repo::MononokeRepo;
pub use crate::repo::Repo;
pub use crate::repo::RepoContext;
pub use crate::repo::StoreRequest;
pub use crate::repo::XRepoLookupExactBehaviour;
pub use crate::repo::XRepoLookupSyncBehaviour;
pub use crate::specifiers::ChangesetId;
pub use crate::specifiers::ChangesetIdPrefix;
pub use crate::specifiers::ChangesetPrefixSpecifier;
pub use crate::specifiers::ChangesetSpecifier;
pub use crate::specifiers::ChangesetSpecifierPrefixResolution;
pub use crate::specifiers::Globalrev;
pub use crate::specifiers::HgChangesetId;
pub use crate::specifiers::HgChangesetIdPrefix;
pub use crate::tree::TreeContext;
pub use crate::tree::TreeEntry;
pub use crate::tree::TreeId;
pub use crate::tree::TreeSummary;
pub use crate::xrepo::CandidateSelectionHintArgs;

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke<R> {
    // Collection of instantiated repos currently being served.
    pub repos: Arc<MononokeRepos<R>>,
    // The collective list of all enabled repos that exist
    // in the current tier (e.g. prod, backup, etc.)
    pub repo_names_in_tier: Vec<String>,
}

impl<R: MononokeRepo> Mononoke<R> {
    /// Create a MononokeAPI instance for MononokeRepos
    ///
    /// Takes extra argument containing list of all aviailable repos
    /// (used to power APIs listing repos; TODO: change that arg to MonononokeConfigs)
    pub fn new(
        repos: Arc<MononokeRepos<R>>,
        repo_names_in_tier: Vec<String>,
    ) -> Result<Self, Error> {
        Ok(Self {
            repos,
            repo_names_in_tier,
        })
    }

    /// Start a request on a repository by name.
    // Method is async and fallible as in the future this may involve
    // instantiating the repo lazily.
    pub async fn repo(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContextBuilder<R>>, MononokeError> {
        match self.repos.get_by_name(name.as_ref()) {
            None => Ok(None),
            Some(repo) => Ok(Some(
                RepoContextBuilder::new(ctx, repo, self.repos.clone()).await?,
            )),
        }
    }

    /// Start a request on a repository by id.
    // Method is async and fallible as in the future this may involve
    // instantiating the repo lazily.
    pub async fn repo_by_id(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<RepoContextBuilder<R>>, MononokeError> {
        match self.repos.get_by_id(repo_id.id()) {
            None => Ok(None),
            Some(repo) => Ok(Some(
                RepoContextBuilder::new(ctx, repo, self.repos.clone()).await?,
            )),
        }
    }

    /// Return the raw underlying repo corresponding to the provided
    /// repo name.
    pub fn raw_repo(&self, name: impl AsRef<str>) -> Option<Arc<R>> {
        self.repos.get_by_name(name.as_ref())
    }

    /// Return the raw underlying repo corresponding to the provided
    /// repo id.
    pub fn raw_repo_by_id(&self, id: i32) -> Option<Arc<R>> {
        self.repos.get_by_id(id)
    }

    /// Get all known repository ids
    pub fn known_repo_ids(&self) -> Vec<RepositoryId> {
        self.repos.iter_ids().map(RepositoryId::new).collect()
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = String> {
        self.repos.iter_names()
    }

    pub fn repos(&self) -> impl Iterator<Item = Arc<R>> {
        self.repos.iter()
    }

    pub fn repo_name_from_id(&self, repo_id: RepositoryId) -> Option<String> {
        self.repos
            .get_by_id(repo_id.id())
            .map(|repo| repo.repo_identity().name().to_string())
    }

    pub fn repo_id_from_name(&self, name: impl AsRef<str>) -> Option<RepositoryId> {
        self.repos
            .get_by_name(name.as_ref())
            .map(|repo| repo.repo_identity().id())
    }

    /// Report configured monitoring stats
    pub async fn report_monitoring_stats(&self, ctx: &CoreContext) -> Result<(), MononokeError> {
        for repo in self.repos.iter() {
            crate::repo::report_monitoring_stats(ctx, &repo).await?;
        }

        Ok(())
    }
}

pub(crate) fn invalid_push_redirected_request(method_name: &str) -> MononokeError {
    MononokeError::InvalidRequest(format!(
        "{method_name} is not supported for push redirected repos"
    ))
}

pub mod test_impl {
    use repo_identity::RepoIdentityRef;

    use super::*;

    impl Mononoke<Repo> {
        /// Create a Mononoke instance for testing.
        pub async fn new_test(
            repos: impl IntoIterator<Item = (String, Repo)>,
        ) -> Result<Self, Error> {
            let repos = repos
                .into_iter()
                .map(|(name, repo)| (repo.repo_identity().id().id(), name, repo))
                .collect::<Vec<_>>();
            let repo_names_in_tier = repos
                .iter()
                .map(|(_, name, _)| name.to_string())
                .collect::<Vec<_>>();
            let mononoke_repos = MononokeRepos::new();
            mononoke_repos.populate(repos);
            Ok(Self {
                repos: Arc::new(mononoke_repos),
                repo_names_in_tier,
            })
        }

        pub async fn new_test_xrepo(small_repo: Repo, large_repo: Repo) -> Result<Self, Error> {
            let repo_names_in_tier = vec![
                small_repo.repo_identity().name().to_string(),
                large_repo.repo_identity().name().to_string(),
            ];
            let mononoke_repos = MononokeRepos::new();
            mononoke_repos.populate(vec![
                (
                    small_repo.repo_identity().id().id(),
                    small_repo.repo_identity().name().to_string(),
                    small_repo,
                ),
                (
                    large_repo.repo_identity().id().id(),
                    large_repo.repo_identity().name().to_string(),
                    large_repo,
                ),
            ]);
            Ok(Self {
                repos: Arc::new(mononoke_repos),
                repo_names_in_tier,
            })
        }
    }
}
