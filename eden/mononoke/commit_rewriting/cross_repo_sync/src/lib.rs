/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmark_renaming::BookmarkRenamer;
use bookmarks::BookmarkName;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use cacheblob::MemcacheOps;
use commit_transformation::rewrite_commit as multi_mover_rewrite_commit;
use commit_transformation::upload_commits;
pub use commit_transformation::CommitRewrittenToEmpty;
use commit_transformation::MultiMover;
use context::CoreContext;
use environment::Caching;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::future;
use futures::future::try_join;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::FutureExt;
use live_commit_sync_config::LiveCommitSyncConfig;
use maplit::hashmap;
use maplit::hashset;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::PushrebaseFlags;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use movers::Mover;
use phases::PhasesRef;
use pushrebase::do_pushrebase_bonsai;
use pushrebase::PushrebaseError;
use reachabilityindex::LeastCommonAncestorsHint;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::info;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use synced_commit_mapping::EquivalentWorkingCopyEntry;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use synced_commit_mapping::SyncedCommitSourceRepo;
use thiserror::Error;
use topo_sort::sort_topological;
use tunables::tunables;

use crate::pushrebase_hook::CrossRepoSyncPushrebaseHook;
use reporting::log_rewrite;
pub use reporting::CommitSyncContext;
use sync_config_version_utils::get_mapping_change_version;
use sync_config_version_utils::get_version;
use sync_config_version_utils::get_version_for_merge;
pub use sync_config_version_utils::CHANGE_XREPO_MAPPING_EXTRA;
use types::Source;
use types::Target;

mod commit_sync_data_provider;
pub mod commit_sync_outcome;
mod pushrebase_hook;
mod reporting;
mod sync_config_version_utils;
pub mod types;
pub mod validation;

pub use crate::commit_sync_outcome::commit_sync_outcome_exists;
pub use crate::commit_sync_outcome::get_commit_sync_outcome;
pub use crate::commit_sync_outcome::get_commit_sync_outcome_with_hint;
pub use crate::commit_sync_outcome::get_plural_commit_sync_outcome;
pub use crate::commit_sync_outcome::CandidateSelectionHint;
pub use crate::commit_sync_outcome::CommitSyncOutcome;
pub use crate::commit_sync_outcome::PluralCommitSyncOutcome;
pub use commit_sync_data_provider::CommitSyncDataProvider;

const LEASE_WARNING_THRESHOLD: Duration = Duration::from_secs(60);

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

/// Create a version of `cs` with `Mover` applied to all changes
/// The return value can be:
/// - `Err` if the rewrite failed
/// - `Ok(None)` if the rewrite decided that this commit should
///              not be present in the rewrite target
/// - `Ok(Some(rewritten))` for a successful rewrite, which should be
///                         present in the rewrite target
/// The notion that the commit "should not be present in the rewrite
/// target" means that the commit is not a merge and all of its changes
/// were rewritten into nothingness by the `Mover`.
///
/// Precondition: this function expects all `cs` parents to be present
/// in `remapped_parents` as keys, and their remapped versions as values.
pub async fn rewrite_commit<'a>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: Mover,
    source_repo: BlobRepo,
    commit_rewritten_to_empty: CommitRewrittenToEmpty,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    multi_mover_rewrite_commit(
        ctx,
        cs,
        remapped_parents,
        mover_to_multi_mover(mover),
        source_repo,
        None,
        commit_rewritten_to_empty,
    )
    .await
}

/// Mover moves a path to at most a single path, while MultiMover can move a
/// path to multiple.
pub fn mover_to_multi_mover(mover: Mover) -> MultiMover {
    Arc::new(move |path: &MPath| -> Result<Vec<MPath>, Error> {
        Ok(mover(path)?.into_iter().collect())
    })
}

async fn remap_parents<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    cs: &BonsaiChangesetMut,
    commit_syncer: &'a CommitSyncer<M>,
    hint: CandidateSelectionHint,
) -> Result<HashMap<ChangesetId, ChangesetId>, Error> {
    let mut remapped_parents = HashMap::new();
    for commit in &cs.parents {
        let maybe_sync_outcome = commit_syncer
            .get_commit_sync_outcome_with_hint(ctx, Source(*commit), hint.clone())
            .await?;
        let sync_outcome: Result<_, Error> =
            maybe_sync_outcome.ok_or_else(|| ErrorKind::ParentNotRemapped(*commit).into());
        let sync_outcome = sync_outcome?;

        use CommitSyncOutcome::*;
        let remapped_parent = match sync_outcome {
            RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => cs_id,
            NotSyncCandidate(_) => {
                return Err(ErrorKind::ParentNotSyncCandidate(*commit).into());
            }
        };

        remapped_parents.insert(*commit, remapped_parent);
    }

    Ok(remapped_parents)
}

#[derive(Clone, Default)]
pub struct SyncedAncestorsVersions {
    // Versions of all synced ancestors
    versions: HashSet<CommitSyncConfigVersion>,
}

impl SyncedAncestorsVersions {
    pub fn has_ancestor_with_a_known_outcome(&self) -> bool {
        !self.versions.is_empty()
    }

    pub fn get_only_version(&self) -> Result<Option<CommitSyncConfigVersion>, Error> {
        let mut iter = self.versions.iter();
        match (iter.next(), iter.next()) {
            (Some(v1), None) => Ok(Some(v1.clone())),
            (None, None) => Err(format_err!("no ancestor version found")),
            _ => Err(format_err!(
                "cannot find single ancestor version: {:?}",
                self.versions
            )),
        }
    }
}

/// Returns unsynced ancestors and also list of CommitSyncConfigVersion
/// of latest *synced* ancestors.
/// See example below (U means unsyned, S means synced)
///
/// ```text
/// U2
/// |
/// U1
/// |
/// S with version V1
/// ```
///
/// In this case we'll return [U1, U2] and \[V1\]
pub async fn find_toposorted_unsynced_ancestors<M>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    start_cs_id: ChangesetId,
) -> Result<(Vec<ChangesetId>, SyncedAncestorsVersions), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let mut synced_ancestors_versions = SyncedAncestorsVersions::default();
    let source_repo = commit_syncer.get_source_repo();

    let mut visited = hashset! { start_cs_id };
    let mut q = VecDeque::new();
    q.push_back(start_cs_id);

    let mut commits_to_backsync = HashMap::new();

    let mut traversed_num = 0;
    while let Some(cs_id) = q.pop_front() {
        traversed_num += 1;
        if traversed_num % 100 == 0 {
            info!(
                ctx.logger(),
                "traversed {} commits while listing unsynced ancestors, starting from {}",
                traversed_num,
                start_cs_id
            );
        }

        let maybe_plural_outcome = commit_syncer
            .get_plural_commit_sync_outcome(ctx, cs_id)
            .await?;

        match maybe_plural_outcome {
            Some(plural) => {
                use PluralCommitSyncOutcome::*;
                match plural {
                    NotSyncCandidate(version) => {
                        synced_ancestors_versions.versions.insert(version);
                    }
                    RewrittenAs(cs_ids_versions) => {
                        for (_, version) in cs_ids_versions {
                            synced_ancestors_versions.versions.insert(version);
                        }
                    }
                    EquivalentWorkingCopyAncestor(_, version) => {
                        synced_ancestors_versions.versions.insert(version);
                    }
                };
                continue;
            }
            None => {
                let maybe_mapping_change =
                    get_mapping_change_version(ctx, commit_syncer.get_source_repo(), cs_id);
                let parents = source_repo.get_changeset_parents_by_bonsai(ctx.clone(), cs_id);
                let (maybe_mapping_change, parents) =
                    try_join(maybe_mapping_change, parents).await?;

                if let Some(version) = maybe_mapping_change {
                    synced_ancestors_versions.versions.insert(version);
                }
                commits_to_backsync.insert(cs_id, parents.clone());

                q.extend(parents.into_iter().filter(|p| visited.insert(*p)));
            }
        }
    }

    // sort_topological returns a list which contains both commits_to_backsync keys and
    // values (i.e. parents). We need only keys, so below we added a filter to remove parents
    //
    // TODO(stash): T60147215 change sort_topological logic to not return parents!
    let res = sort_topological(&commits_to_backsync).expect("unexpected cycle in commit graph!");

    Ok((
        res.into_iter()
            .filter(|r| commits_to_backsync.contains_key(r))
            .collect(),
        synced_ancestors_versions,
    ))
}

#[derive(Clone)]
pub enum CommitSyncRepos {
    LargeToSmall {
        large_repo: BlobRepo,
        small_repo: BlobRepo,
    },
    SmallToLarge {
        small_repo: BlobRepo,
        large_repo: BlobRepo,
    },
}

impl CommitSyncRepos {
    /// Create a new instance of `CommitSyncRepos`
    /// Whether it's SmallToLarge or LargeToSmall is determined by
    /// source_repo/target_repo and common_commit_sync_config.
    pub fn new(
        source_repo: BlobRepo,
        target_repo: BlobRepo,
        common_commit_sync_config: &CommonCommitSyncConfig,
    ) -> Result<Self, Error> {
        let small_repo_id = if common_commit_sync_config.large_repo_id == source_repo.get_repoid()
            && common_commit_sync_config
                .small_repos
                .contains_key(&target_repo.get_repoid())
        {
            target_repo.get_repoid()
        } else if common_commit_sync_config.large_repo_id == target_repo.get_repoid()
            && common_commit_sync_config
                .small_repos
                .contains_key(&source_repo.get_repoid())
        {
            source_repo.get_repoid()
        } else {
            return Err(format_err!(
                "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
                source_repo.get_repoid(),
                target_repo.get_repoid()
            ));
        };

        if source_repo.get_repoid() == small_repo_id {
            Ok(CommitSyncRepos::SmallToLarge {
                large_repo: target_repo,
                small_repo: source_repo,
            })
        } else {
            Ok(CommitSyncRepos::LargeToSmall {
                large_repo: source_repo,
                small_repo: target_repo,
            })
        }
    }
}

pub fn create_commit_syncer_lease(
    fb: FacebookInit,
    caching: Caching,
) -> Result<Arc<dyn LeaseOps>, Error> {
    if let Caching::Enabled(_) = caching {
        Ok(Arc::new(MemcacheOps::new(fb, "x-repo-sync-lease", "")?))
    } else {
        Ok(Arc::new(InProcessLease::new()))
    }
}

#[derive(Clone)]
pub struct CommitSyncer<M> {
    // TODO: Finish refactor and remove pub
    pub mapping: M,
    pub repos: CommitSyncRepos,
    pub commit_sync_data_provider: CommitSyncDataProvider,
    pub scuba_sample: MononokeScubaSampleBuilder,
    pub x_repo_sync_lease: Arc<dyn LeaseOps>,
}

impl<M> fmt::Debug for CommitSyncer<M>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source_repo_id = self.get_source_repo_id();
        let target_repo_id = self.get_target_repo_id();
        write!(f, "CommitSyncer{{{}->{}}}", source_repo_id, target_repo_id)
    }
}

impl<M> CommitSyncer<M>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    pub fn new(
        ctx: &CoreContext,
        mapping: M,
        repos: CommitSyncRepos,
        live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
        lease: Arc<dyn LeaseOps>,
    ) -> Self {
        let commit_sync_data_provider = CommitSyncDataProvider::Live(live_commit_sync_config);
        Self::new_with_provider_impl(ctx, mapping, repos, commit_sync_data_provider, lease)
    }

    pub fn new_with_provider(
        ctx: &CoreContext,
        mapping: M,
        repos: CommitSyncRepos,
        commit_sync_data_provider: CommitSyncDataProvider,
    ) -> Self {
        Self::new_with_provider_impl(
            ctx,
            mapping,
            repos,
            commit_sync_data_provider,
            Arc::new(InProcessLease::new()),
        )
    }

    fn new_with_provider_impl(
        ctx: &CoreContext,
        mapping: M,
        repos: CommitSyncRepos,
        commit_sync_data_provider: CommitSyncDataProvider,
        x_repo_sync_lease: Arc<dyn LeaseOps>,
    ) -> Self {
        let scuba_sample = reporting::get_scuba_sample(
            ctx,
            repos.get_source_repo().name(),
            repos.get_target_repo().name(),
        );
        Self {
            mapping,
            repos,
            commit_sync_data_provider,
            scuba_sample,
            x_repo_sync_lease,
        }
    }

    pub fn get_source_repo(&self) -> &BlobRepo {
        self.repos.get_source_repo()
    }

    pub fn get_source_repo_id(&self) -> RepositoryId {
        self.get_source_repo().get_repoid()
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        self.repos.get_target_repo()
    }

    pub fn get_target_repo_id(&self) -> RepositoryId {
        self.get_target_repo().get_repoid()
    }

    pub fn get_source_repo_type(&self) -> SyncedCommitSourceRepo {
        self.repos.get_source_repo_type()
    }

    pub fn get_large_repo(&self) -> &BlobRepo {
        use CommitSyncRepos::*;
        match self.repos {
            LargeToSmall { ref large_repo, .. } => large_repo,
            SmallToLarge { ref large_repo, .. } => large_repo,
        }
    }

    pub fn get_small_repo(&self) -> &BlobRepo {
        use CommitSyncRepos::*;
        match self.repos {
            LargeToSmall { ref small_repo, .. } => small_repo,
            SmallToLarge { ref small_repo, .. } => small_repo,
        }
    }

    pub fn get_mapping(&self) -> &M {
        &self.mapping
    }

    pub fn get_commit_sync_data_provider(&self) -> &CommitSyncDataProvider {
        &self.commit_sync_data_provider
    }

    pub async fn version_exists(&self, version: &CommitSyncConfigVersion) -> Result<bool, Error> {
        self.commit_sync_data_provider
            .version_exists(self.get_target_repo_id(), version)
            .await
    }

    pub async fn get_mover_by_version(
        &self,
        version: &CommitSyncConfigVersion,
    ) -> Result<Mover, Error> {
        let (source_repo, target_repo) = self.get_source_target();
        self.commit_sync_data_provider
            .get_mover(version, source_repo.get_repoid(), target_repo.get_repoid())
            .await
    }

    pub async fn get_reverse_mover_by_version(
        &self,
        version: &CommitSyncConfigVersion,
    ) -> Result<Mover, Error> {
        let (source_repo, target_repo) = self.get_source_target();
        self.commit_sync_data_provider
            .get_reverse_mover(version, source_repo.get_repoid(), target_repo.get_repoid())
            .await
    }

    pub async fn get_bookmark_renamer(&self) -> Result<BookmarkRenamer, Error> {
        let (source_repo, target_repo) = self.get_source_target();

        self.commit_sync_data_provider
            .get_bookmark_renamer(source_repo.get_repoid(), target_repo.get_repoid())
            .await
    }

    pub async fn get_reverse_bookmark_renamer(&self) -> Result<BookmarkRenamer, Error> {
        let (source_repo, target_repo) = self.get_source_target();

        self.commit_sync_data_provider
            .get_reverse_bookmark_renamer(source_repo.get_repoid(), target_repo.get_repoid())
            .await
    }

    pub async fn rename_bookmark(
        &self,
        bookmark: &BookmarkName,
    ) -> Result<Option<BookmarkName>, Error> {
        Ok(self.get_bookmark_renamer().await?(bookmark))
    }

    pub async fn get_plural_commit_sync_outcome<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<Option<PluralCommitSyncOutcome>, Error> {
        get_plural_commit_sync_outcome(
            ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            Source(source_cs_id),
            &self.mapping,
            self.repos.get_direction(),
            &self.commit_sync_data_provider,
        )
        .await
    }

    pub async fn get_commit_sync_outcome<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<Option<CommitSyncOutcome>, Error> {
        get_commit_sync_outcome(
            ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            Source(source_cs_id),
            &self.mapping,
            self.repos.get_direction(),
            &self.commit_sync_data_provider,
        )
        .await
    }

    pub async fn commit_sync_outcome_exists<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: Source<ChangesetId>,
    ) -> Result<bool, Error> {
        commit_sync_outcome_exists(
            ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            source_cs_id,
            &self.mapping,
            self.repos.get_direction(),
            &self.commit_sync_data_provider,
        )
        .await
    }

    pub async fn get_commit_sync_outcome_with_hint<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: Source<ChangesetId>,
        hint: CandidateSelectionHint,
    ) -> Result<Option<CommitSyncOutcome>, Error> {
        get_commit_sync_outcome_with_hint(
            ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            source_cs_id,
            &self.mapping,
            hint,
            self.repos.get_direction(),
            &self.commit_sync_data_provider,
        )
        .await
    }

    // This is the function that safely syncs a commit and all of its unsynced ancestors from a
    // source repo to target repo. If commit is already synced then it just does a lookup.
    // But safety comes with flexibility cost - not all of the syncs are allowed. For example,
    // syncing a *public* commit from a small repo to a large repo is not allowed:
    // 1) If small repo is the source of truth, then there should be only a single job that
    //    does this sync. Since this function can be used from many places and we have no
    //    way of ensuring only a single job does the sync, this sync is forbidden completely.
    // 2) If large repo is a source of truth, then there should never be a case with public
    //    commit in a small repo not having an equivalent in the large repo.
    pub async fn sync_commit(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        ancestor_selection_hint: CandidateSelectionHint,
        commit_sync_context: CommitSyncContext,
    ) -> Result<Option<ChangesetId>, Error> {
        let before = Instant::now();
        let res = self
            .sync_commit_impl(ctx, source_cs_id, ancestor_selection_hint)
            .await;
        let elapsed = before.elapsed();
        log_rewrite(
            ctx,
            self.scuba_sample.clone(),
            source_cs_id,
            "sync_commit",
            commit_sync_context,
            elapsed,
            &res,
        );
        res
    }

    async fn sync_commit_impl(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        ancestor_selection_hint: CandidateSelectionHint,
    ) -> Result<Option<ChangesetId>, Error> {
        let (unsynced_ancestors, synced_ancestors_versions) =
            find_toposorted_unsynced_ancestors(ctx, self, source_cs_id).await?;

        let source_repo = self.repos.get_source_repo();
        let target_repo = self.repos.get_target_repo();

        let small_repo = self.get_small_repo();
        let source_repo_is_small = source_repo.get_repoid() == small_repo.get_repoid();

        if source_repo_is_small {
            let public_unsynced_ancestors = source_repo
                .phases()
                .get_public(
                    ctx,
                    unsynced_ancestors.clone(),
                    false, /* ephemeral_derive */
                )
                .await?;
            if !public_unsynced_ancestors.is_empty() {
                return Err(format_err!(
                    "unexpected sync lookup attempt - trying to sync \
                     a public commit from small repo to a large repo. Syncing public commits is \
                     only supported from a large repo to a small repo"
                ));
            }
        }

        for ancestor in unsynced_ancestors {
            let lease_key = format!(
                "sourcerepo_{}_targetrepo_{}.{}",
                source_repo.get_repoid().id(),
                target_repo.get_repoid().id(),
                source_cs_id,
            );

            let checker = || async {
                let maybe_outcome = self.get_commit_sync_outcome(ctx, source_cs_id).await?;
                Result::<_, Error>::Ok(maybe_outcome.is_some())
            };
            let sync = || async {
                let parents = self
                    .get_source_repo()
                    .get_changeset_fetcher()
                    .get_parents(ctx.clone(), ancestor)
                    .await?;
                if parents.is_empty() {
                    let version = self
                        .get_version_for_syncing_commit_with_no_parent(
                            ctx,
                            ancestor,
                            &synced_ancestors_versions,
                        )
                        .await
                        .with_context(|| {
                            format_err!("failed to sync ancestor {} of {}", ancestor, source_cs_id)
                        })?;

                    self.unsafe_sync_commit_impl(
                        ctx,
                        ancestor,
                        ancestor_selection_hint.clone(),
                        Some(version),
                    )
                    .await?;
                } else {
                    self.unsafe_sync_commit_impl(
                        ctx,
                        ancestor,
                        ancestor_selection_hint.clone(),
                        None,
                    )
                    .await?;
                }
                Ok(())
            };

            if tunables().get_xrepo_disable_commit_sync_lease() {
                sync().await?;
            } else {
                run_with_lease(ctx, &self.x_repo_sync_lease, lease_key, checker, sync).await?;
            }
        }

        let commit_sync_outcome = self
            .get_commit_sync_outcome(ctx, source_cs_id)
            .await?
            .ok_or_else(|| format_err!("was not able to remap a commit {}", source_cs_id))?;
        use CommitSyncOutcome::*;
        let res = match commit_sync_outcome {
            NotSyncCandidate(_) => None,
            RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => Some(cs_id),
        };
        Ok(res)
    }

    // Get a version to use while syncing ancestor with no parent  of `source_cs_id`
    // We only allow syncing such commits if we an unambiguously decide on the CommitSyncConfig version to use,
    // and we do that by ensuring that there is exactly one unique version among the commit sync outcomes
    // of all the already-synced ancestors of `source_cs_id`
    async fn get_version_for_syncing_commit_with_no_parent(
        &self,
        ctx: &CoreContext,
        commit_with_no_parent: ChangesetId,
        synced_ancestors_versions: &SyncedAncestorsVersions,
    ) -> Result<CommitSyncConfigVersion, Error> {
        let maybe_version =
            get_version(ctx, self.get_source_repo(), commit_with_no_parent, vec![]).await?;
        let version = match maybe_version {
            Some(version) => version,
            None => synced_ancestors_versions
                .get_only_version()?
                .ok_or_else(|| format_err!("no versions found for {}", commit_with_no_parent))?,
        };
        Ok(version)
    }

    /// Create a changeset, equivalent to `source_cs_id` in the target repo
    /// The difference between this function and `rewrite_commit` is that
    /// `rewrite_commit` does not know anything about the repo and only produces
    /// a `BonsaiChangesetMut` object, which later may or may not be uploaded
    /// into the repository.
    /// This function is prefixed with unsafe because it requires that ancestors commits are
    /// already synced and because syncing commit public commits from a small repo to a large repo
    /// using this function might lead to repo corruption.
    /// `parent_selection_hint` is used when remapping this commit's parents.
    /// See `CandidateSelectionHint` doctring for more details.
    pub async fn unsafe_sync_commit(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        parent_mapping_selection_hint: CandidateSelectionHint,
        commit_sync_context: CommitSyncContext,
    ) -> Result<Option<ChangesetId>, Error> {
        let before = Instant::now();
        let res = self
            .unsafe_sync_commit_impl(ctx, source_cs_id, parent_mapping_selection_hint, None)
            .await;
        let elapsed = before.elapsed();
        log_rewrite(
            ctx,
            self.scuba_sample.clone(),
            source_cs_id,
            "unsafe_sync_commit",
            commit_sync_context,
            elapsed,
            &res,
        );
        res
    }

    /// Just like unsafe_sync_commit, but sets an expected version i.e.
    /// for commits that have at least a single parent it checks that these commits
    /// will be rewritten with this version, and for commits with no parents
    /// this expected version will be used for rewriting.
    pub async fn unsafe_sync_commit_with_expected_version(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        parent_mapping_selection_hint: CandidateSelectionHint,
        expected_version: CommitSyncConfigVersion,
        commit_sync_context: CommitSyncContext,
    ) -> Result<Option<ChangesetId>, Error> {
        let before = Instant::now();
        let res = self
            .unsafe_sync_commit_impl(
                ctx,
                source_cs_id,
                parent_mapping_selection_hint,
                Some(expected_version),
            )
            .await;
        let elapsed = before.elapsed();
        log_rewrite(
            ctx,
            self.scuba_sample.clone(),
            source_cs_id,
            "unsafe_sync_commit_with_expected_version",
            commit_sync_context,
            elapsed,
            &res,
        );
        res
    }

    async fn unsafe_sync_commit_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
        parent_mapping_selection_hint: CandidateSelectionHint,
        expected_version: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        // Take most of below function unsafe_sync_commit into here and delete. Leave pushrebase in next fn
        let (source_repo, _) = self.get_source_target();

        debug!(
            ctx.logger(),
            "{:?}: unsafe_sync_commit called for {}, with hint: {:?}",
            self,
            source_cs_id,
            parent_mapping_selection_hint
        );

        let cs = source_cs_id.load(ctx, source_repo.blobstore()).await?;
        let parents: Vec<_> = cs.parents().collect();

        if parents.is_empty() {
            match expected_version {
                Some(version) => self.sync_commit_no_parents(ctx, cs, version).await,
                None => Err(format_err!(
                    "no version specified for remapping commit {} with no parents",
                    source_cs_id
                )),
            }
        } else if parents.len() == 1 {
            self.sync_commit_single_parent(ctx, cs, parent_mapping_selection_hint, expected_version)
                .await
        } else {
            self.sync_merge(ctx, cs, expected_version).await
        }
    }

    /// Rewrite a commit and creates in target repo if parents are already created.
    /// This is marked as unsafe since it might lead to repo corruption if used incorrectly.
    /// It can be used to import a merge commit from a new repo:
    ///
    /// ```text
    ///     source repo:
    ///
    ///     O  <- master (common bookmark). Points to a merge commit that imports a new repo
    ///     | \
    ///     O   \
    ///          O  <- merge commit in the new repo we are trying to merge into master.
    ///         /  \   naive_sync_commit can be used to sync this commit
    /// ```
    ///
    /// Normally this function is able to find the parents for the synced commit automatically
    /// but in case it can't then `maybe_parents` parameter allows us to overwrite parents of
    /// the synced commit.
    pub async fn unsafe_always_rewrite_sync_commit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
        maybe_parents: Option<HashMap<ChangesetId, ChangesetId>>,
        sync_config_version: &CommitSyncConfigVersion,
        commit_sync_context: CommitSyncContext,
    ) -> Result<Option<ChangesetId>, Error> {
        let before = Instant::now();
        let res = self
            .unsafe_always_rewrite_sync_commit_impl(
                ctx,
                source_cs_id,
                maybe_parents,
                sync_config_version,
            )
            .await;
        let elapsed = before.elapsed();
        log_rewrite(
            ctx,
            self.scuba_sample.clone(),
            source_cs_id,
            "unsafe_always_rewrite_sync_commit",
            commit_sync_context,
            elapsed,
            &res,
        );
        res
    }

    async fn unsafe_always_rewrite_sync_commit_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
        maybe_parents: Option<HashMap<ChangesetId, ChangesetId>>,
        sync_config_version: &CommitSyncConfigVersion,
    ) -> Result<Option<ChangesetId>, Error> {
        let (source_repo, target_repo) = self.get_source_target();
        let mover = self.get_mover_by_version(sync_config_version).await?;
        let source_cs = source_cs_id.load(ctx, source_repo.blobstore()).await?;

        let source_cs = source_cs.clone().into_mut();
        let remapped_parents = match maybe_parents {
            Some(parents) => parents,
            None => remap_parents(ctx, &source_cs, self, CandidateSelectionHint::Only).await?, // TODO: check if only is ok
        };

        let rewritten_commit = rewrite_commit(
            ctx,
            source_cs,
            &remapped_parents,
            mover,
            source_repo.clone(),
            CommitRewrittenToEmpty::Discard,
        )
        .await?;
        match rewritten_commit {
            None => {
                self.set_no_sync_candidate(ctx, source_cs_id, sync_config_version.clone())
                    .await?;
                Ok(None)
            }
            Some(rewritten) => {
                // Sync commit
                let frozen = rewritten.freeze()?;
                let frozen_cs_id = frozen.get_changeset_id();
                upload_commits(ctx, vec![frozen], &source_repo, &target_repo).await?;

                update_mapping_with_version(
                    ctx,
                    hashmap! { source_cs_id => frozen_cs_id },
                    self,
                    sync_config_version,
                )
                .await?;
                Ok(Some(frozen_cs_id))
            }
        }
    }

    /// This function is prefixed with unsafe because it requires that ancestors commits are
    /// already synced and because there should be exactly one sync job that uses this function
    /// for a (small repo -> large repo) pair.
    pub async fn unsafe_sync_commit_pushrebase<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs: BonsaiChangeset,
        bookmark: BookmarkName,
        target_lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>>,
        commit_sync_context: CommitSyncContext,
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = source_cs.get_changeset_id();
        let before = Instant::now();
        let res = self
            .unsafe_sync_commit_pushrebase_impl(ctx, source_cs, bookmark, target_lca_hint)
            .await;
        let elapsed = before.elapsed();

        log_rewrite(
            ctx,
            self.scuba_sample.clone(),
            source_cs_id,
            "unsafe_sync_commit_pushrebase",
            commit_sync_context,
            elapsed,
            &res,
        );
        res
    }

    pub async fn get_common_pushrebase_bookmarks(&self) -> Result<Vec<BookmarkName>, Error> {
        self.commit_sync_data_provider
            .get_common_pushrebase_bookmarks(self.get_small_repo().get_repoid())
            .await
    }

    async fn unsafe_sync_commit_pushrebase_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs: BonsaiChangeset,
        bookmark: BookmarkName,
        target_lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>>,
    ) -> Result<Option<ChangesetId>, Error> {
        let hash = source_cs.get_changeset_id();
        let (source_repo, target_repo) = self.get_source_target();

        let parent_selection_hint = CandidateSelectionHint::OnlyOrAncestorOfBookmark(
            Target(bookmark.clone()),
            Target(self.get_target_repo().clone()),
            target_lca_hint,
        );

        let mut remapped_parents_outcome = vec![];
        for p in source_cs.parents() {
            let maybe_commit_sync_outcome = self
                .get_commit_sync_outcome_with_hint(ctx, Source(p), parent_selection_hint.clone())
                .await?
                .map(|sync_outcome| (sync_outcome, p));
            let commit_sync_outcome = maybe_commit_sync_outcome.ok_or_else(|| {
                format_err!(
                    "parent {} has not been remapped yet, therefore can't remap {}",
                    p,
                    source_cs.get_changeset_id()
                )
            })?;
            remapped_parents_outcome.push(commit_sync_outcome);
        }

        let p1 = remapped_parents_outcome.get(0);
        let p2 = remapped_parents_outcome.get(1);
        let version_name = match (p1, p2) {
            (None, None) => {
                return Err(format_err!("cannot pushrebase a commit with no parents"));
            }
            (Some((sync_outcome, _)), None) => {
                use CommitSyncOutcome::*;

                let version_name = match sync_outcome {
                    NotSyncCandidate(_) => {
                        return Err(ErrorKind::ParentNotSyncCandidate(hash).into());
                    }
                    RewrittenAs(_, version_name)
                    | EquivalentWorkingCopyAncestor(_, version_name) => version_name.clone(),
                };

                let maybe_version =
                    get_version(ctx, self.get_source_repo(), hash, &[version_name]).await?;
                maybe_version.ok_or_else(|| {
                    format_err!("unexpected can not find commit sync version for {}", hash)
                })?
            }
            _ => {
                // FIXME: Had to turn it to a vector to avoid "One type is more general than the other"
                // errors
                let outcomes = remapped_parents_outcome
                    .iter()
                    .map(|(outcome, _)| outcome)
                    .collect::<Vec<_>>();
                get_version_for_merge(ctx, self.get_source_repo(), hash, outcomes).await?
            }
        };

        let mover = self.get_mover_by_version(&version_name).await?;
        let source_cs_mut = source_cs.clone().into_mut();
        let remapped_parents =
            remap_parents(ctx, &source_cs_mut, self, parent_selection_hint).await?;
        let rewritten = rewrite_commit(
            ctx,
            source_cs_mut,
            &remapped_parents,
            mover,
            source_repo.clone(),
            CommitRewrittenToEmpty::Discard,
        )
        .await?;

        match rewritten {
            None => {
                if remapped_parents_outcome.is_empty() {
                    self.set_no_sync_candidate(ctx, hash, version_name).await?;
                } else if remapped_parents_outcome.len() == 1 {
                    use CommitSyncOutcome::*;
                    let (sync_outcome, _) = &remapped_parents_outcome[0];
                    match sync_outcome {
                        NotSyncCandidate(version) => {
                            self.set_no_sync_candidate(ctx, hash, version.clone())
                                .await?;
                        }
                        RewrittenAs(cs_id, version)
                        | EquivalentWorkingCopyAncestor(cs_id, version) => {
                            self.update_wc_equivalence_with_version(
                                ctx,
                                hash,
                                Some(*cs_id),
                                version.clone(),
                            )
                            .await?;
                        }
                    };
                } else {
                    return Err(ErrorKind::AmbiguousWorkingCopyEquivalent(
                        source_cs.get_changeset_id(),
                    )
                    .into());
                }

                Ok(None)
            }
            Some(rewritten) => {
                // Sync commit
                let frozen = rewritten.freeze()?;
                let rewritten_list = hashset![frozen];
                upload_commits(
                    ctx,
                    rewritten_list.clone().into_iter().collect(),
                    &source_repo,
                    &target_repo,
                )
                .await?;

                let pushrebase_flags = {
                    let mut flags = PushrebaseFlags::default();
                    flags.rewritedates = false;
                    flags.forbid_p2_root_rebases = false;
                    flags.casefolding_check = false;
                    flags.recursion_limit = None;
                    flags
                };

                let pushrebase_res = do_pushrebase_bonsai(
                    ctx,
                    &target_repo,
                    &pushrebase_flags,
                    &bookmark,
                    &rewritten_list,
                    &[CrossRepoSyncPushrebaseHook::new(
                        hash,
                        self.repos.clone(),
                        version_name.clone(),
                    )],
                )
                .await;
                let pushrebase_res =
                    pushrebase_res.map_err(|e| Error::from(ErrorKind::PushrebaseFailure(e)))?;
                let pushrebased_changeset = pushrebase_res.head;
                Ok(Some(pushrebased_changeset))
            }
        }
    }

    async fn sync_commit_no_parents<'a>(
        &'a self,
        ctx: &'a CoreContext,
        cs: BonsaiChangeset,
        expected_version: CommitSyncConfigVersion,
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = cs.get_changeset_id();
        let maybe_version = get_version(ctx, self.get_source_repo(), source_cs_id, &[]).await?;
        if let Some(version) = maybe_version {
            if version != expected_version {
                return Err(format_err!(
                    "computed sync config version {} for {} not the same as expected version {}",
                    source_cs_id,
                    version,
                    expected_version
                ));
            }
        }

        let (source_repo, target_repo) = self.get_source_target();
        let mover = self.get_mover_by_version(&expected_version).await?;

        match rewrite_commit(
            ctx,
            cs.into_mut(),
            &HashMap::new(),
            mover,
            source_repo.clone(),
            CommitRewrittenToEmpty::Discard,
        )
        .await?
        {
            Some(rewritten) => {
                let frozen = rewritten.freeze()?;
                upload_commits(ctx, vec![frozen.clone()], &source_repo, &target_repo).await?;

                // update_mapping also updates working copy equivalence, so no need
                // to do it separately
                update_mapping_with_version(
                    ctx,
                    hashmap! { source_cs_id => frozen.get_changeset_id() },
                    self,
                    &expected_version,
                )
                .await?;
                Ok(Some(frozen.get_changeset_id()))
            }
            None => {
                self.update_wc_equivalence_with_version(ctx, source_cs_id, None, expected_version)
                    .await?;
                Ok(None)
            }
        }
    }

    async fn sync_commit_single_parent<'a>(
        &'a self,
        ctx: &'a CoreContext,
        cs: BonsaiChangeset,
        parent_mapping_selection_hint: CandidateSelectionHint,
        expected_version: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = cs.get_changeset_id();
        let cs = cs.into_mut();
        let p = cs.parents[0];
        let (source_repo, target_repo) = self.get_source_target();

        let maybe_parent_sync_outcome = self
            .get_commit_sync_outcome_with_hint(ctx, Source(p), parent_mapping_selection_hint)
            .await?;

        let parent_sync_outcome = maybe_parent_sync_outcome
            .ok_or_else(|| format_err!("Parent commit {} is not synced yet", p))?;

        use CommitSyncOutcome::*;
        match parent_sync_outcome {
            NotSyncCandidate(version) => {
                // If there's not working copy for parent commit then there's no working
                // copy for child either.
                self.set_no_sync_candidate(ctx, source_cs_id, version)
                    .await?;
                Ok(None)
            }
            RewrittenAs(remapped_p, version)
            | EquivalentWorkingCopyAncestor(remapped_p, version) => {
                let maybe_version =
                    get_version(ctx, self.get_source_repo(), source_cs_id, &[version]).await?;
                let version = maybe_version.ok_or_else(|| {
                    format_err!("sync config version not found for {}", source_cs_id)
                })?;

                if let Some(expected_version) = expected_version {
                    if expected_version != version {
                        return Err(ErrorKind::UnexpectedVersion {
                            expected_version,
                            actual_version: version,
                            cs_id: source_cs_id,
                        }
                        .into());
                    }
                }

                let rewrite_paths = self.get_mover_by_version(&version).await?;

                let mut remapped_parents = HashMap::new();
                remapped_parents.insert(p, remapped_p);

                // If a commit is changing mapping let's always rewrite it to
                // small repo regardless if outcome is empty. This is to ensure
                // that efter changing mapping there's a commit in small repo
                // with new mapping on top.
                let maybe_mapping_change_version =
                    get_mapping_change_version(ctx, self.get_source_repo(), source_cs_id).await?;
                let discard_commits_rewriting_to_empty = if maybe_mapping_change_version.is_some() {
                    CommitRewrittenToEmpty::Keep
                } else {
                    CommitRewrittenToEmpty::Discard
                };

                let maybe_rewritten = rewrite_commit(
                    ctx,
                    cs,
                    &remapped_parents,
                    rewrite_paths,
                    source_repo.clone(),
                    discard_commits_rewriting_to_empty,
                )
                .await?;
                match maybe_rewritten {
                    Some(rewritten) => {
                        let frozen = rewritten.freeze()?;
                        upload_commits(ctx, vec![frozen.clone()], &source_repo, &target_repo)
                            .await?;

                        // update_mapping also updates working copy equivalence, so no need
                        // to do it separately
                        update_mapping_with_version(
                            ctx,
                            hashmap! { source_cs_id => frozen.get_changeset_id() },
                            self,
                            &version,
                        )
                        .await?;
                        Ok(Some(frozen.get_changeset_id()))
                    }
                    None => {
                        // Source commit doesn't rewrite to any target commits.
                        // In that case equivalent working copy is the equivalent working
                        // copy of the parent
                        self.update_wc_equivalence_with_version(
                            ctx,
                            source_cs_id,
                            Some(remapped_p),
                            version,
                        )
                        .await?;
                        Ok(None)
                    }
                }
            }
        }
    }

    /// Get `CommitSyncConfigVersion` to use while remapping a
    /// merge commit (`source_cs_id`)
    /// The idea is to derive this version from the `parent_outcomes`
    /// according to the following rules:
    /// - all `NotSyncCandidate` parents are ignored
    /// - all `RewrittenAs` and `EquivalentWorkingCopyAncestor`
    ///   parents have the same (non-None) version associated
    async fn get_mover_to_use_for_merge<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
        parent_outcomes: Vec<&CommitSyncOutcome>,
    ) -> Result<(Mover, CommitSyncConfigVersion), Error> {
        let version =
            get_version_for_merge(ctx, self.get_source_repo(), source_cs_id, parent_outcomes)
                .await?;

        let mover = self
            .get_mover_by_version(&version)
            .await
            .with_context(|| format!("failed getting a mover of version {}", version))?;
        Ok((mover, version))
    }

    // See more details about the algorithm in https://fb.quip.com/s8fYAOxEohtJ
    // A few important notes:
    // 1) Merges are synced only in LARGE -> SMALL direction.
    // 2) If a large repo merge has any parent after big merge, then this merge will appear
    //    in all small repos
    async fn sync_merge<'a>(
        &'a self,
        ctx: &'a CoreContext,
        cs: BonsaiChangeset,
        expected_version: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        if let CommitSyncRepos::SmallToLarge { .. } = self.repos {
            bail!("syncing merge commits is supported only in large to small direction");
        }

        let source_cs_id = cs.get_changeset_id();
        let cs = cs.into_mut();

        let parent_outcomes = stream::iter(cs.parents.clone().into_iter().map(|p| {
            self.get_commit_sync_outcome(ctx, p).and_then(
                move |maybe_outcome| match maybe_outcome {
                    Some(outcome) => future::ok((p, outcome)),
                    None => future::err(format_err!("{} does not have CommitSyncOutcome", p)),
                },
            )
        }));

        let sync_outcomes = parent_outcomes
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?;

        // At this point we know that there's at least one parent after big merge. However we still
        // might have a parent that's NotSyncCandidate
        //
        //   B
        //   | \
        //   |  \
        //   R   X  <- new repo was merged, however this repo was not synced at all.
        //   |   |
        //   |   ...
        //   ...
        //   BM  <- Big merge
        //  / \
        //  ...
        //
        // This parents will be completely removed. However when these parents are removed
        // we also need to be careful to strip all copy info

        let mut not_sync_candidate_versions = HashSet::new();

        let new_parents: HashMap<_, _> = sync_outcomes
            .iter()
            .filter_map(|(p, outcome)| {
                use CommitSyncOutcome::*;
                match outcome {
                    EquivalentWorkingCopyAncestor(cs_id, _) | RewrittenAs(cs_id, _) => {
                        Some((*p, *cs_id))
                    }
                    NotSyncCandidate(version) => {
                        not_sync_candidate_versions.insert(version);
                        None
                    }
                }
            })
            .collect();

        let cs = self.strip_removed_parents(cs, new_parents.keys().collect())?;

        if !new_parents.is_empty() {
            // FIXME: Had to turn it to a vector to avoid "One type is more general than the other"
            // errors
            let outcomes = sync_outcomes
                .iter()
                .map(|(_, outcome)| outcome)
                .collect::<Vec<_>>();

            let (mover, version) = self
                .get_mover_to_use_for_merge(ctx, source_cs_id, outcomes)
                .await
                .context("failed getting a mover to use for merge rewriting")?;

            if let Some(expected_version) = expected_version {
                if version != expected_version {
                    return Err(ErrorKind::UnexpectedVersion {
                        expected_version,
                        actual_version: version,
                        cs_id: source_cs_id,
                    }
                    .into());
                }
            }

            match rewrite_commit(
                ctx,
                cs,
                &new_parents,
                mover,
                self.get_source_repo().clone(),
                CommitRewrittenToEmpty::Discard,
            )
            .await?
            {
                Some(rewritten) => {
                    let target_cs_id = self
                        .upload_rewritten_and_update_mapping(ctx, source_cs_id, rewritten, version)
                        .await?;
                    Ok(Some(target_cs_id))
                }
                None => {
                    // We should end up in this branch only if we have a single
                    // parent, because merges are never skipped during rewriting
                    let parent_cs_id = new_parents
                        .values()
                        .next()
                        .ok_or_else(|| Error::msg("logic merge: cannot find merge parent"))?;
                    self.update_wc_equivalence_with_version(
                        ctx,
                        source_cs_id,
                        Some(*parent_cs_id),
                        version,
                    )
                    .await?;
                    Ok(Some(*parent_cs_id))
                }
            }
        } else {
            // All parents of the merge commit are NotSyncCandidate, mark it as NotSyncCandidate
            // as well
            let mut iter = not_sync_candidate_versions.iter();
            let version = match (iter.next(), iter.next()) {
                (Some(_v1), Some(_v2)) => {
                    return Err(format_err!(
                        "Too many parent NotSyncCandidate versions: {:?} while syncing {}",
                        not_sync_candidate_versions,
                        source_cs_id
                    ));
                }
                (Some(version), None) => version,
                _ => {
                    return Err(format_err!(
                        "Can't find parent version for merge commit {}",
                        source_cs_id
                    ));
                }
            };

            self.set_no_sync_candidate(ctx, source_cs_id, (*version).clone())
                .await?;
            Ok(None)
        }
    }

    // Rewrites a commit and uploads it
    async fn upload_rewritten_and_update_mapping<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_cs_id: ChangesetId,
        rewritten: BonsaiChangesetMut,
        version: CommitSyncConfigVersion,
    ) -> Result<ChangesetId, Error> {
        let (source_repo, target_repo) = self.get_source_target();

        let frozen = rewritten.freeze()?;
        let target_cs_id = frozen.get_changeset_id();
        upload_commits(ctx, vec![frozen], &source_repo, &target_repo).await?;

        // update_mapping also updates working copy equivalence, so no need
        // to do it separately
        update_mapping_with_version(
            ctx,
            hashmap! { source_cs_id =>  target_cs_id},
            self,
            &version,
        )
        .await?;
        Ok(target_cs_id)
    }

    // Some of the parents were removed - we need to remove copy-info that's not necessary
    // anymore
    fn strip_removed_parents(
        &self,
        mut source_cs: BonsaiChangesetMut,
        new_source_parents: Vec<&ChangesetId>,
    ) -> Result<BonsaiChangesetMut, Error> {
        source_cs
            .parents
            .retain(|p| new_source_parents.contains(&&*p));

        for (_, file_change) in source_cs.file_changes.iter_mut() {
            match file_change {
                FileChange::Change(ref mut tc) => match tc.copy_from() {
                    Some((_, parent)) if !new_source_parents.contains(&parent) => {
                        *tc = tc.with_new_copy_from(None);
                    }
                    _ => {}
                },
                FileChange::Deletion
                | FileChange::UntrackedDeletion
                | FileChange::UntrackedChange(_) => {}
            }
        }

        Ok(source_cs)
    }

    fn get_source_target(&self) -> (BlobRepo, BlobRepo) {
        match self.repos.clone() {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                ..
            } => (large_repo, small_repo),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                ..
            } => (small_repo, large_repo),
        }
    }

    async fn set_no_sync_candidate<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_bcs_id: ChangesetId,
        version_name: CommitSyncConfigVersion,
    ) -> Result<(), Error> {
        self.update_wc_equivalence_with_version(ctx, source_bcs_id, None, version_name)
            .await
    }

    async fn update_wc_equivalence_with_version<'a>(
        &'a self,
        ctx: &'a CoreContext,
        source_bcs_id: ChangesetId,
        maybe_target_bcs_id: Option<ChangesetId>,
        version_name: CommitSyncConfigVersion,
    ) -> Result<(), Error> {
        if tunables().get_xrepo_sync_disable_all_syncs() {
            return Err(ErrorKind::XRepoSyncDisabled.into());
        }

        let CommitSyncer { repos, mapping, .. } = self.clone();
        let (source_repo, target_repo, source_is_large) = match repos {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                ..
            } => (large_repo, small_repo, true),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                ..
            } => (small_repo, large_repo, false),
        };

        let source_repoid = source_repo.get_repoid();
        let target_repoid = target_repo.get_repoid();

        let wc_entry = match maybe_target_bcs_id {
            Some(target_bcs_id) => {
                if source_is_large {
                    EquivalentWorkingCopyEntry {
                        large_repo_id: source_repoid,
                        large_bcs_id: source_bcs_id,
                        small_repo_id: target_repoid,
                        small_bcs_id: Some(target_bcs_id),
                        version_name: Some(version_name),
                    }
                } else {
                    EquivalentWorkingCopyEntry {
                        large_repo_id: target_repoid,
                        large_bcs_id: target_bcs_id,
                        small_repo_id: source_repoid,
                        small_bcs_id: Some(source_bcs_id),
                        version_name: Some(version_name),
                    }
                }
            }
            None => {
                if !source_is_large {
                    bail!(
                        "unexpected wc equivalence update: small repo commit should always remap to large repo"
                    );
                }
                EquivalentWorkingCopyEntry {
                    large_repo_id: source_repoid,
                    large_bcs_id: source_bcs_id,
                    small_repo_id: target_repoid,
                    small_bcs_id: None,
                    version_name: Some(version_name),
                }
            }
        };

        mapping
            .insert_equivalent_working_copy(ctx, wc_entry)
            .await
            .map(|_| ())
    }
}

impl CommitSyncRepos {
    pub fn get_source_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall { large_repo, .. } => large_repo,
            CommitSyncRepos::SmallToLarge { small_repo, .. } => small_repo,
        }
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall { small_repo, .. } => small_repo,
            CommitSyncRepos::SmallToLarge { large_repo, .. } => large_repo,
        }
    }

    pub fn get_source_repo_type(&self) -> SyncedCommitSourceRepo {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => SyncedCommitSourceRepo::Large,
            CommitSyncRepos::SmallToLarge { .. } => SyncedCommitSourceRepo::Small,
        }
    }

    fn get_direction(&self) -> CommitSyncDirection {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => CommitSyncDirection::LargeToSmall,
            CommitSyncRepos::SmallToLarge { .. } => CommitSyncDirection::SmallToLarge,
        }
    }
}

pub async fn update_mapping_with_version<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: &'a CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    syncer: &'a CommitSyncer<M>,
    version_name: &CommitSyncConfigVersion,
) -> Result<(), Error> {
    if tunables().get_xrepo_sync_disable_all_syncs() {
        return Err(ErrorKind::XRepoSyncDisabled.into());
    }

    let entries: Vec<_> = mapped
        .into_iter()
        .map(|(from, to)| {
            create_synced_commit_mapping_entry(from, to, &syncer.repos, version_name.clone())
        })
        .collect();

    syncer.mapping.add_bulk(ctx, entries).await?;
    Ok(())
}

pub fn create_synced_commit_mapping_entry(
    from: ChangesetId,
    to: ChangesetId,
    repos: &CommitSyncRepos,
    version_name: CommitSyncConfigVersion,
) -> SyncedCommitMappingEntry {
    let (source_repo, target_repo, source_is_large) = match repos {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
            ..
        } => (large_repo, small_repo, true),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
            ..
        } => (small_repo, large_repo, false),
    };

    let source_repoid = source_repo.get_repoid();
    let target_repoid = target_repo.get_repoid();

    if source_is_large {
        SyncedCommitMappingEntry::new(
            source_repoid,
            from,
            target_repoid,
            to,
            version_name,
            repos.get_source_repo_type(),
        )
    } else {
        SyncedCommitMappingEntry::new(
            target_repoid,
            to,
            source_repoid,
            from,
            version_name,
            repos.get_source_repo_type(),
        )
    }
}

#[derive(Clone)]
pub struct Syncers<M: SyncedCommitMapping + Clone + 'static> {
    pub large_to_small: CommitSyncer<M>,
    pub small_to_large: CommitSyncer<M>,
}

pub fn create_commit_syncers<M>(
    ctx: &CoreContext,
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    mapping: M,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    x_repo_sync_lease: Arc<dyn LeaseOps>,
) -> Result<Syncers<M>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let common_config = live_commit_sync_config.get_common_config(large_repo.get_repoid())?;

    let small_to_large_commit_sync_repos =
        CommitSyncRepos::new(small_repo.clone(), large_repo.clone(), &common_config)?;
    let large_to_small_commit_sync_repos =
        CommitSyncRepos::new(large_repo, small_repo, &common_config)?;

    let large_to_small_commit_syncer = CommitSyncer::new(
        ctx,
        mapping.clone(),
        large_to_small_commit_sync_repos,
        live_commit_sync_config.clone(),
        x_repo_sync_lease.clone(),
    );
    let small_to_large_commit_syncer = CommitSyncer::new(
        ctx,
        mapping,
        small_to_large_commit_sync_repos,
        live_commit_sync_config,
        x_repo_sync_lease,
    );

    Ok(Syncers {
        large_to_small: large_to_small_commit_syncer,
        small_to_large: small_to_large_commit_syncer,
    })
}

async fn run_with_lease<CheckerFunc, CheckerFut, Func, Fut>(
    ctx: &CoreContext,
    lease: &Arc<dyn LeaseOps>,
    lease_key: String,
    checker: CheckerFunc,
    func: Func,
) -> Result<(), Error>
where
    CheckerFunc: Fn() -> CheckerFut,
    CheckerFut: futures::Future<Output = Result<bool, Error>>,
    Func: Fn() -> Fut,
    Fut: futures::Future<Output = Result<(), Error>>,
{
    let lease_start = Instant::now();
    let mut logged_slow_lease = false;
    let lease_key = Arc::new(lease_key);

    let mut backoff_ms = 200;
    loop {
        if checker().await? {
            // The operation was already done, nothing to do
            break;
        }

        let leased = if tunables().get_xrepo_disable_commit_sync_lease() {
            true
        } else {
            let result = lease.try_add_put_lease(&lease_key).await;
            // In case of lease unavailability assume it's taken to not block the backsyncer
            result.unwrap_or(true)
        };

        if !leased {
            let elapsed = lease_start.elapsed();
            if elapsed >= LEASE_WARNING_THRESHOLD && !logged_slow_lease {
                logged_slow_lease = true;
                ctx.scuba()
                    .clone()
                    .add("x_repo_sync_lease_wait", elapsed.as_secs())
                    .log_with_msg("Slow x-repo sync lease", None);
            }
            // Didn't get the lease - wait a little bit and retry
            let sleep = rand::random::<u64>() % backoff_ms;
            tokio::time::sleep(Duration::from_millis(sleep)).await;

            backoff_ms = std::cmp::min(1000, backoff_ms * 2);
            continue;
        }

        // We have the lease and commit is not synced - let's sync it
        let (sender, receiver) = oneshot::channel();
        scopeguard::defer! {
            let _ = sender.send(());
        };
        lease.renew_lease_until(ctx.clone(), &lease_key, receiver.map(|_| ()).boxed());

        func().await?;
        break;
    }

    Ok(())
}
