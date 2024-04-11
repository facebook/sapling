/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use bookmarks::BookmarkKey;
use borrowed::borrowed;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use cacheblob::MemcacheOps;
use changeset_info::ChangesetInfo;
use commit_transformation::rewrite_commit_with_file_changes_filter;
use commit_transformation::FileChangeFilter;
use commit_transformation::FileChangeFilterApplication;
use commit_transformation::FileChangeFilterFunc;
use commit_transformation::MultiMover;
use commit_transformation::RewriteOpts;
use context::CoreContext;
use derived_data::BonsaiDerived;
use environment::Caching;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::future::try_join;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::FutureExt;
use live_commit_sync_config::LiveCommitSyncConfig;
use maplit::hashset;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::GitSubmodulesChangesAction;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use movers::Mover;
use slog::debug;
use slog::info;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use synced_commit_mapping::SyncedCommitSourceRepo;
use topo_sort::sort_topological;

use crate::commit_sync_config_utils::get_mover;
use crate::commit_sync_outcome::CandidateSelectionHint;
use crate::commit_sync_outcome::CommitSyncOutcome;
use crate::commit_sync_outcome::DesiredRelationship;
use crate::commit_sync_outcome::PluralCommitSyncOutcome;
use crate::commit_syncer::CommitSyncer;
use crate::git_submodules::expand_and_validate_all_git_submodule_file_changes;
use crate::git_submodules::SubmoduleExpansionData;
use crate::sync_config_version_utils::get_mapping_change_version;
use crate::types::ErrorKind;
use crate::types::Repo;
use crate::types::Source;
use crate::types::SubmoduleDeps;
use crate::types::Target;
use crate::CommitSyncContext;

const LEASE_WARNING_THRESHOLD: Duration = Duration::from_secs(60);

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
pub async fn rewrite_commit<'a, R: Repo>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: Mover,
    source_repo: &'a R,
    rewrite_opts: RewriteOpts,
    git_submodules_action: GitSubmodulesChangesAction,
    mb_submodule_expansion_data: Option<SubmoduleExpansionData<'a, R>>,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    // TODO(T169695293): add filter to only keep submodules for implicit deletes?
    let (file_changes_filters, cs): (Vec<FileChangeFilter<'a>>, BonsaiChangesetMut) =
        match git_submodules_action {
            GitSubmodulesChangesAction::Strip => {
                let filter_func: FileChangeFilterFunc<'a> = Arc::new(move |(_path, fc)| match fc {
                    FileChange::Change(tfc) => tfc.file_type() != FileType::GitSubmodule,
                    _ => true,
                });
                let filter: FileChangeFilter<'a> = FileChangeFilter {
                    func: filter_func,
                    application: FileChangeFilterApplication::MultiMover,
                };

                (vec![filter], cs)
            }
            // Keep submodules -> no filters and keep original bonsai
            GitSubmodulesChangesAction::Keep => (vec![], cs),
            // Expand submodules -> no filters, but modify the file change
            // file types in the bonsai
            GitSubmodulesChangesAction::Expand => {
                let submodule_expansion_data = mb_submodule_expansion_data.ok_or(anyhow!("Submodule expansion data not provided when submodules is enabled for small repo"))?;

                let new_bonsai = expand_and_validate_all_git_submodule_file_changes(
                    ctx,
                    cs,
                    source_repo,
                    submodule_expansion_data,
                    mover.clone(),
                    remapped_parents,
                    rewrite_opts,
                )
                .await?;

                return Ok(Some(new_bonsai));
            }
        };

    rewrite_commit_with_file_changes_filter(
        ctx,
        cs,
        remapped_parents,
        mover_to_multi_mover(mover),
        source_repo,
        None,
        rewrite_opts,
        file_changes_filters,
    )
    .await
}

/// Mover moves a path to at most a single path, while MultiMover can move a
/// path to multiple.
pub fn mover_to_multi_mover(mover: Mover) -> MultiMover<'static> {
    Arc::new(
        move |path: &NonRootMPath| -> Result<Vec<NonRootMPath>, Error> {
            Ok(mover(path)?.into_iter().collect())
        },
    )
}

pub(crate) async fn remap_parents<'a, M: SyncedCommitMapping + Clone + 'static, R: Repo>(
    ctx: &CoreContext,
    cs: &BonsaiChangesetMut,
    commit_syncer: &'a CommitSyncer<M, R>,
    hint: CandidateSelectionHint<R>,
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

#[derive(Clone, Default, Debug)]
pub struct SyncedAncestorsVersions {
    // Versions of all synced ancestors
    pub versions: HashSet<CommitSyncConfigVersion>,
    // Rewritten ancestors: source_cs_id -> (rewritten_cs_id, version)
    pub rewritten_ancestors: HashMap<ChangesetId, (ChangesetId, CommitSyncConfigVersion)>,
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
pub async fn find_toposorted_unsynced_ancestors<M, R>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M, R>,
    start_cs_id: ChangesetId,
    desired_relationship: Option<DesiredRelationship<R>>,
) -> Result<(Vec<ChangesetId>, SyncedAncestorsVersions), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
    R: Repo,
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
        let maybe_plural_outcome = match (maybe_plural_outcome.clone(), &desired_relationship) {
            (Some(plural), Some(desired_relationship)) => {
                let outcome = plural
                    .filter_by_desired_relationship(ctx, desired_relationship)
                    .await?;
                match outcome {
                    PluralCommitSyncOutcome::RewrittenAs(plural) if plural.is_empty() => None,
                    _ => Some(outcome),
                }
            }
            _ => maybe_plural_outcome,
        };

        match maybe_plural_outcome {
            Some(plural) => {
                use PluralCommitSyncOutcome::*;
                match plural {
                    NotSyncCandidate(version) => {
                        synced_ancestors_versions.versions.insert(version);
                    }
                    RewrittenAs(cs_ids_versions) => {
                        for (rewritten_cs_id, version) in cs_ids_versions {
                            synced_ancestors_versions.versions.insert(version.clone());
                            synced_ancestors_versions
                                .rewritten_ancestors
                                .insert(cs_id, (rewritten_cs_id, version));
                        }
                    }
                    EquivalentWorkingCopyAncestor(_, version) => {
                        synced_ancestors_versions.versions.insert(version);
                    }
                };
                continue;
            }
            None => {
                let maybe_mapping_change = async move {
                    get_mapping_change_version(
                        &ChangesetInfo::derive(ctx, commit_syncer.get_source_repo(), cs_id).await?,
                    )
                };
                let parents = source_repo.commit_graph().changeset_parents(ctx, cs_id);
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
    let res = sort_topological(&commits_to_backsync).expect("unexpected cycle in commit graph!");

    Ok((
        res.into_iter()
            .filter(|r| commits_to_backsync.contains_key(r))
            .collect(),
        synced_ancestors_versions,
    ))
}

/// Same as `find_toposorted_unsynced_ancestors` but uses the skew binary commit
/// graph to find the oldest unsynced ancestor quicker.
/// NOTE: because this is used to run initial imports of small repos into large
/// repos, this function DOES NOT take into account hardcoded mappings in
/// hg extra metadata, as `find_toposorted_unsynced_ancestors` does.
pub async fn find_toposorted_unsynced_ancestors_with_commit_graph<'a, M, R>(
    ctx: &'a CoreContext,
    commit_syncer: &'a CommitSyncer<M, R>,
    start_cs_id: ChangesetId,
) -> Result<(Vec<ChangesetId>, SyncedAncestorsVersions)>
where
    M: SyncedCommitMapping + Clone + 'static,
    R: Repo,
{
    let source_repo = commit_syncer.get_source_repo();

    let commit_graph = source_repo.commit_graph();

    // Monotonic property function that will be used to traverse the commit
    // graph to find the latest synced ancestors (if any).
    let is_synced = |cs_id: ChangesetId| {
        borrowed!(ctx, commit_syncer);

        async move {
            let maybe_plural_outcome = commit_syncer
                .get_plural_commit_sync_outcome(ctx, cs_id)
                .await?;

            match maybe_plural_outcome {
                Some(_plural) => Ok(true),
                None => Ok(false),
            }
        }
    };

    let synced_ancestors_frontier = commit_graph
        .ancestors_frontier_with(ctx, vec![start_cs_id], is_synced)
        .await?;

    // Get the config versions from all synced ancestors
    let synced_ancestors_list = stream::iter(&synced_ancestors_frontier)
        .then(|cs_id| {
            borrowed!(ctx, commit_syncer);

            async move {
                let maybe_plural_outcome = commit_syncer
                    .get_plural_commit_sync_outcome(ctx, *cs_id)
                    .await?;

                match maybe_plural_outcome {
                    Some(plural) => {
                        use PluralCommitSyncOutcome::*;
                        match plural {
                            NotSyncCandidate(version) => Ok(vec![(*cs_id, (None, version))]),
                            RewrittenAs(cs_ids_versions) => Ok(cs_ids_versions
                                .into_iter()
                                .map(|(rewritten_cs_id, v)| (*cs_id, (Some(rewritten_cs_id), v)))
                                .collect()),
                            EquivalentWorkingCopyAncestor(equivalent_cs_id, version) => {
                                Ok(vec![(*cs_id, (Some(equivalent_cs_id), version))])
                            }
                        }
                    }
                    None => Err(anyhow!("Failed to get config version from synced ancestor")),
                }
            }
        })
        .try_collect::<HashSet<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<(ChangesetId, (Option<ChangesetId>, CommitSyncConfigVersion))>>();

    let synced_ancestors_versions = synced_ancestors_list
        .iter()
        .map(|(_source, (_target, v))| v.clone())
        .collect();
    let rewritten_ancestors = synced_ancestors_list
        .into_iter()
        .filter_map(|(source, (maybe_target, version))| {
            maybe_target.map(|target| (source, (target, version)))
        })
        .collect();

    // Get the oldest unsynced ancestors by getting the difference between the
    // ancestors from the starting changeset and its synced ancestors.
    let mut commits_to_sync = commit_graph
        .ancestors_difference(ctx, vec![start_cs_id], synced_ancestors_frontier)
        .await?;

    // `ancestors_difference` returns the commits in reverse topological order
    commits_to_sync.reverse();

    Ok((
        commits_to_sync,
        SyncedAncestorsVersions {
            versions: synced_ancestors_versions,
            rewritten_ancestors,
        },
    ))
}

/// Finds what's the "current" version for large repo (it may have been updated since last
/// pushrebase), and returns the version and the mapping of the synced ancestors to the
/// more-up-to-date changesets with equivalent working copy id.
///
/// This is written with assumption of no diamond merges (which are not supported by other parts of
/// x_repo_sync) and that small repo bookmark is never moving backwards (which is not supported by
/// other pieces of the infra).
pub async fn get_version_and_parent_map_for_sync_via_pushrebase<
    'a,
    M: SyncedCommitMapping + Clone + 'static,
    R,
>(
    ctx: &'a CoreContext,
    commit_syncer: &CommitSyncer<M, R>,
    target_bookmark: &Target<BookmarkKey>,
    parent_version: CommitSyncConfigVersion,
    synced_ancestors_versions: &SyncedAncestorsVersions,
) -> Result<(CommitSyncConfigVersion, HashMap<ChangesetId, ChangesetId>), Error>
where
    R: Repo,
{
    // Killswitch to disable this logic alltogether.
    if let Ok(true) = justknobs::eval(
        "scm/mononoke:xrepo_disable_forward_sync_over_mapping_change",
        None,
        None,
    ) {
        return Ok((parent_version, HashMap::new()));
    }
    let target_repo = commit_syncer.get_target_repo();
    // Value for the target bookmark. This is not a part of transaction and we're ok with the fact
    // it might be a bit stale.
    let target_bookmark_csid = target_repo
        .bookmarks()
        .get(ctx.clone(), &target_bookmark.0)
        .await?
        .ok_or_else(|| anyhow!("Bookmark {} does not exist", target_bookmark.0))?;

    let target_bookmark_version = if let Some(target_bookmark_version) = target_repo
        .repo_cross_repo()
        .synced_commit_mapping()
        .get_large_repo_commit_version(ctx, target_repo.repo_identity().id(), target_bookmark_csid)
        .await?
    {
        target_bookmark_version
    } else {
        debug!(
            ctx.logger(),
            "target bookmark version: none, parent version: {}", parent_version,
        );
        // If we don't have a version for the target bookmark, we can't do anything.
        return Ok((parent_version, HashMap::new()));
    };
    debug!(
        ctx.logger(),
        "target bookmark version: {}, parent version: {}", target_bookmark_version, parent_version,
    );

    if parent_version == target_bookmark_version {
        // If the parent version is the same as the target bookmark version we don't need
        // to be smart: we can just use the parent version.
        return Ok((parent_version, HashMap::new()));
    }

    // Let's first validate that the target bookmark is still working-copy equivalent to what the
    // parent of the commit we'd like to sync
    let backsyncer = commit_syncer.reverse()?;
    let small_csid_equivalent_to_target_bookmark = if let Some(
        small_csid_equivalent_to_target_bookmark,
    ) = backsyncer
        .sync_commit(
            ctx,
            target_bookmark_csid,
            CandidateSelectionHint::Only,
            CommitSyncContext::XRepoSyncJob,
            false,
        )
        .await?
    {
        small_csid_equivalent_to_target_bookmark
    } else {
        debug!(
            ctx.logger(),
            "target bookmark is not wc-equivalent to synced commit, falling back to parent_version",
        );
        return Ok((parent_version, HashMap::new()));
    };

    let mut parent_mapping = HashMap::new();
    for (source_parent_csid, (target_parent_csid, _version)) in
        synced_ancestors_versions.rewritten_ancestors.iter()
    {
        // If the bookmark value is descendant of our parent it should have equivalent working
        // copy.
        if target_repo
            .commit_graph()
            .is_ancestor(ctx, *target_parent_csid, target_bookmark_csid)
            .await?
            && small_csid_equivalent_to_target_bookmark == *source_parent_csid
        {
            parent_mapping.insert(*target_parent_csid, target_bookmark_csid);
        }
    }
    debug!(ctx.logger(), "parent_mapping: {:?}", parent_mapping,);

    if parent_mapping.is_empty() {
        // None of the parents are ancestors of current position of target_bookmark. Perhaps
        // our view of target bookmark is stale. It's better to avoid changing version.
        debug!(
            ctx.logger(),
            "parent mapping is empty, falling back to parent_version",
        );
        Ok((parent_version, parent_mapping))
    } else if parent_mapping.len() == 1 {
        debug!(
            ctx.logger(),
            "all validations passed, using target_bookmark_version: {}", target_bookmark_version,
        );
        // There's exactly one parent that's ancestor of target_bookmark.
        // let's assume that the target_bookmark is still equivalent to what it represents.
        Ok((target_bookmark_version, parent_mapping))
    } else {
        // There are at least two synced parents that are ancestors of target_bookmark. This
        // practically mean we have a diamond merge at hand.
        Err(anyhow!(
            "Diamond merges are not supported for pushrebase sync"
        ))
    }
}

#[derive(Clone)]
pub enum CommitSyncRepos<R> {
    LargeToSmall {
        large_repo: R,
        small_repo: R,
    },
    SmallToLarge {
        small_repo: R,
        large_repo: R,
        submodule_deps: SubmoduleDeps<R>,
    },
}

impl<R: Repo> CommitSyncRepos<R> {
    /// Create a new instance of `CommitSyncRepos`
    /// Whether it's SmallToLarge or LargeToSmall is determined by
    /// source_repo/target_repo and common_commit_sync_config.
    pub fn new(
        source_repo: R,
        target_repo: R,
        submodule_deps: SubmoduleDeps<R>,
        common_commit_sync_config: &CommonCommitSyncConfig,
    ) -> Result<Self, Error> {
        let small_repo_id = if common_commit_sync_config.large_repo_id
            == source_repo.repo_identity().id()
            && common_commit_sync_config
                .small_repos
                .contains_key(&target_repo.repo_identity().id())
        {
            target_repo.repo_identity().id()
        } else if common_commit_sync_config.large_repo_id == target_repo.repo_identity().id()
            && common_commit_sync_config
                .small_repos
                .contains_key(&source_repo.repo_identity().id())
        {
            source_repo.repo_identity().id()
        } else {
            return Err(format_err!(
                "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
                source_repo.repo_identity().id(),
                target_repo.repo_identity().id()
            ));
        };

        if source_repo.repo_identity().id() == small_repo_id {
            Ok(CommitSyncRepos::SmallToLarge {
                large_repo: target_repo,
                small_repo: source_repo,
                submodule_deps,
            })
        } else {
            Ok(CommitSyncRepos::LargeToSmall {
                large_repo: source_repo,
                small_repo: target_repo,
            })
        }
    }

    // Builds the repos that can be used for opposite sync direction.
    // Note: doesn't support large-to-small as input right now
    pub fn reverse(&self) -> Result<Self> {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => Err(anyhow!(
                "reversing sync direction is only supported for small to large sync (because of submodule dependencies)"
            )),
            CommitSyncRepos::SmallToLarge {
                large_repo,
                small_repo,
                ..
            } => Ok(CommitSyncRepos::LargeToSmall {
                large_repo: large_repo.clone(),
                small_repo: small_repo.clone(),
            }),
        }
    }

    pub fn get_submodule_deps(&self) -> &SubmoduleDeps<R> {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => &SubmoduleDeps::NotNeeded,
            CommitSyncRepos::SmallToLarge { submodule_deps, .. } => submodule_deps,
        }
    }

    pub fn get_source_repo(&self) -> &R {
        match self {
            CommitSyncRepos::LargeToSmall { large_repo, .. } => large_repo,
            CommitSyncRepos::SmallToLarge { small_repo, .. } => small_repo,
        }
    }

    pub fn get_target_repo(&self) -> &R {
        match self {
            CommitSyncRepos::LargeToSmall { small_repo, .. } => small_repo,
            CommitSyncRepos::SmallToLarge { large_repo, .. } => large_repo,
        }
    }

    pub(crate) fn get_source_repo_type(&self) -> SyncedCommitSourceRepo {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => SyncedCommitSourceRepo::Large,
            CommitSyncRepos::SmallToLarge { .. } => SyncedCommitSourceRepo::Small,
        }
    }

    pub(crate) fn get_direction(&self) -> CommitSyncDirection {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => CommitSyncDirection::LargeToSmall,
            CommitSyncRepos::SmallToLarge { .. } => CommitSyncDirection::SmallToLarge,
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

// Some of the parents were removed - we need to remove copy-info that's not necessary
// anymore
pub(crate) fn strip_removed_parents(
    mut source_cs: BonsaiChangesetMut,
    new_source_parents: Vec<&ChangesetId>,
) -> Result<BonsaiChangesetMut, Error> {
    source_cs
        .parents
        .retain(|p| new_source_parents.contains(&p));

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

pub(crate) async fn get_mover_by_version(
    version: &CommitSyncConfigVersion,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    source_id: Source<RepositoryId>,
    target_repo_id: Target<RepositoryId>,
) -> Result<Mover, Error> {
    get_mover(
        live_commit_sync_config,
        version,
        source_id.0,
        target_repo_id.0,
    )
    .await
}

pub async fn update_mapping_with_version<'a, M: SyncedCommitMapping + Clone + 'static, R: Repo>(
    ctx: &'a CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    syncer: &'a CommitSyncer<M, R>,
    version_name: &CommitSyncConfigVersion,
) -> Result<(), Error> {
    let xrepo_sync_disable_all_syncs =
        justknobs::eval("scm/mononoke:xrepo_sync_disable_all_syncs", None, None)
            .unwrap_or_default();
    if xrepo_sync_disable_all_syncs {
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

pub fn create_synced_commit_mapping_entry<R: Repo>(
    from: ChangesetId,
    to: ChangesetId,
    repos: &CommitSyncRepos<R>,
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

    let source_repoid = source_repo.repo_identity().id();
    let target_repoid = target_repo.repo_identity().id();

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
pub struct Syncers<M: SyncedCommitMapping + Clone + 'static, R: Repo> {
    pub large_to_small: CommitSyncer<M, R>,
    pub small_to_large: CommitSyncer<M, R>,
}

pub fn create_commit_syncers<M, R>(
    ctx: &CoreContext,
    small_repo: R,
    large_repo: R,
    // Map from submodule path in the repo to the submodule's Mononoke repo
    // instance.
    submodule_deps: SubmoduleDeps<R>,
    mapping: M,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    x_repo_sync_lease: Arc<dyn LeaseOps>,
) -> Result<Syncers<M, R>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
    R: Repo,
{
    let common_config =
        live_commit_sync_config.get_common_config(large_repo.repo_identity().id())?;

    let small_to_large_commit_sync_repos = CommitSyncRepos::new(
        small_repo.clone(),
        large_repo.clone(),
        submodule_deps,
        &common_config,
    )?;
    let large_to_small_commit_sync_repos = CommitSyncRepos::new(
        large_repo,
        small_repo,
        SubmoduleDeps::NotNeeded,
        &common_config,
    )?;

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

pub(crate) async fn run_with_lease<CheckerFunc, CheckerFut, Func, Fut>(
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

        let leased = if justknobs::eval("scm/mononoke:xrepo_disable_commit_sync_lease", None, None)
            .unwrap_or_default()
        {
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

/// Get the prefix used to generate the submodule metadata file name from the
/// small repo sync config.
pub async fn get_x_repo_submodule_metadata_file_prefx_from_config(
    small_repo_id: RepositoryId,
    config_version: &CommitSyncConfigVersion,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<String> {
    // Get the full commit sync config for that version name.
    let mut commit_sync_config = live_commit_sync_config
        .get_commit_sync_config_by_version(small_repo_id, config_version)
        .await?;

    // Get the small repo sync config for the repo we're syncing
    let small_repo_sync_config = commit_sync_config
        .small_repos
        .remove(&small_repo_id)
        .ok_or(
            anyhow!(
                "Small repo config for repo with id {} not found in commit sync config with version {} ",
                small_repo_id,
                config_version.0
            )
        )?;

    Ok(small_repo_sync_config
        .submodule_config
        .submodule_metadata_file_prefix)
}
