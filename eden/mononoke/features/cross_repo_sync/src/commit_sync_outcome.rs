/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Error;
use anyhow::anyhow;
use bookmarks::BookmarkKey;
use context::CoreContext;
use futures::Future;
use futures::TryFutureExt;
use futures::future::try_join_all;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use slog::debug;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::WorkingCopyEquivalence;

use crate::commit_sync_config_utils::get_small_repos_for_version;
use crate::types::Repo;
use crate::types::Source;
use crate::types::Target;

/// The state of a source repo commit in a target repo, assuming
/// that any multiple `RewrittenAs` options have been resolved
/// into a single one
#[derive(Clone, Debug, PartialEq)]
pub enum CommitSyncOutcome {
    /// Not suitable for syncing to this repo
    NotSyncCandidate(CommitSyncConfigVersion),
    /// This commit is a 1:1 semantic mapping, but sync process rewrote it to a new ID.
    RewrittenAs(ChangesetId, CommitSyncConfigVersion),
    /// This commit is removed by the sync process, and the commit with the given ID has same content
    EquivalentWorkingCopyAncestor(ChangesetId, CommitSyncConfigVersion),
}

/// The state of a source repo commit in a target repo, which
/// allows for multiple `RewrittenAs` options
#[derive(Debug, PartialEq, Clone)]
pub enum PluralCommitSyncOutcome {
    /// Not suitable for syncing to this repo
    NotSyncCandidate(CommitSyncConfigVersion),
    /// This commit maps to several other commits in the target repo
    RewrittenAs(Vec<(ChangesetId, CommitSyncConfigVersion)>),
    /// This commit is removed by the sync process, and the commit with the given ID has same content
    EquivalentWorkingCopyAncestor(ChangesetId, CommitSyncConfigVersion),
}

/// A hint to the synced commit selection algorithm
/// Most often used to disambiguate the mapping that has many commits corresponding to the same commit in
/// the source repo. If there's a single option it's usually the one used (hence it's "hint", not "requirement")
///
/// See the docstring for `get_plural_commit_sync_outcome`
/// for why this is needed.
#[derive(Clone)]
pub enum CandidateSelectionHint<R: Repo> {
    /// Selected candidate should be the only candidate
    Only,
    /// Selected candidate should be a given changeset
    Exact(Target<ChangesetId>),
    /// Selected candidate should be an ancestor of a given bookmark
    AncestorOfBookmark(Target<BookmarkKey>, Target<R>),
    /// Selected candidate should be a descendant of a given bookmark
    DescendantOfBookmark(Target<BookmarkKey>, Target<R>),
    /// Selected candidate should be an ancestor of a given changeset
    AncestorOfCommit(Target<ChangesetId>, Target<R>),
    /// Selected candidate should be a descendant of a given changeset
    DescendantOfCommit(Target<ChangesetId>, Target<R>),
}

impl<R: Repo> fmt::Debug for CandidateSelectionHint<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Only => write!(f, "CandidateSelectionHint::Only"),
            Self::Exact(cs_id) => write!(f, "CandidateSelectionHint::Exact({})", cs_id.0),
            Self::AncestorOfBookmark(bn, _) => {
                write!(f, "CandidateSelectionHint::AncestorOfBookmark({})", bn.0)
            }
            Self::DescendantOfBookmark(bn, _) => {
                write!(f, "CandidateSelectionHint::DescendantOfBookmark({})", bn.0)
            }
            Self::AncestorOfCommit(cs_id, _) => {
                write!(f, "CandidateSelectionHint::AncestorOfCommit({})", cs_id.0)
            }
            Self::DescendantOfCommit(cs_id, _) => {
                write!(f, "CandidateSelectionHint::DescendantOfCommit({})", cs_id.0)
            }
        }
    }
}

impl<R: Repo> CandidateSelectionHint<R> {
    /// Convert `self` into an appropriate variant of the `DesiredRelationship`
    /// if it is possible. Specifically:
    /// - `Only` variant does not represent a topological relationship, so cannot
    ///   be converted into `DesiredRelationship`
    /// - `Exact` variant represents `DesiredRelationship::EqualTo`
    /// - `AncestorOfCommit` and `DescendantOfCommit` translate into
    ///   corresponding `DesiredRelationship` variants
    /// - `AncestorOfBookmark` and `DescendantOfBookmark` behave either
    ///   as their commit counterparts (if the bookmark exists), or as `Only` (otherwise)
    ///
    /// Note that hints, which refer to bookmarks may not be fully valid at the time
    /// of use: specifically, the bookmark may not exist. That should not be considered
    /// a "hard failure", as a hint may be used for bookmark creation, or at the time when
    /// bookmark was already deleted. Instead, for these cases the idea is to just
    /// "downgrade" a hint to be an equivalent of `Only` and fail on multiple candidates.
    pub async fn try_into_desired_relationship(
        self,
        ctx: &CoreContext,
    ) -> Result<Option<DesiredRelationship<R>>, Error> {
        match self {
            Self::Only => Ok(None),
            Self::Exact(cs_id) => Ok(Some(DesiredRelationship::EqualTo(cs_id))),
            Self::AncestorOfBookmark(bookmark, target_repo) => {
                // Bookmark absence is not a failure, see doctring
                let maybe_target_cs_id: Option<Target<ChangesetId>> = target_repo
                    .0
                    .bookmarks()
                    .get(ctx.clone(), &bookmark.0, bookmarks::Freshness::MostRecent)
                    .await?
                    .map(Target);

                Ok(maybe_target_cs_id
                    .map(|target_cs_id| DesiredRelationship::AncestorOf(target_cs_id, target_repo)))
            }
            Self::DescendantOfBookmark(bookmark, target_repo) => {
                // Bookmark absence is not a failure, see doctring
                let maybe_target_cs_id: Option<Target<ChangesetId>> = target_repo
                    .0
                    .bookmarks()
                    .get(ctx.clone(), &bookmark.0, bookmarks::Freshness::MostRecent)
                    .await?
                    .map(Target);

                Ok(maybe_target_cs_id.map(|target_cs_id| {
                    DesiredRelationship::DescendantOf(target_cs_id, target_repo)
                }))
            }
            Self::AncestorOfCommit(target_cs_id, target_repo) => Ok(Some(
                DesiredRelationship::AncestorOf(target_cs_id, target_repo),
            )),
            Self::DescendantOfCommit(target_cs_id, target_repo) => Ok(Some(
                DesiredRelationship::DescendantOf(target_cs_id, target_repo),
            )),
        }
    }
}

/// Get `PluralCommitSyncOutcome` for `source_cs_id`
/// This is a building block for other outcome-producing functions
/// Note: it is possible to have multiple commit sync outcomes
/// for a given commit in a small-to-large direction. An example
/// of such situation is:
/// ```text
/// A  D   E
/// |  |   |
/// B  C   |
///  \/    |
///  X     Y
///  |     |
///  |   small repo
/// large repo
/// ```
/// If we assume that:
/// - `X` is an equivalent of `Y`
/// - `B` and `C` don't touch any files form the small repo (`NotSyncCandidate`)
/// - `A` and `D` are fully identical with the exception of their parent commits
///
/// Then both `A` and `D` are in `PluralCommitSyncOutcome::RewrittenAs` of `E`
pub async fn get_plural_commit_sync_outcome<'a, M: SyncedCommitMapping>(
    ctx: &'a CoreContext,
    source_repo_id: Source<RepositoryId>,
    target_repo_id: Target<RepositoryId>,
    source_cs_id: Source<ChangesetId>,
    mapping: &'a M,
    direction: CommitSyncDirection,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<Option<PluralCommitSyncOutcome>, Error> {
    let remapped = mapping
        .get(ctx, source_repo_id.0, source_cs_id.0, target_repo_id.0)
        .await?;
    if !remapped.is_empty() {
        let remapped: Result<Vec<_>, Error> = remapped.into_iter()
            .map(|(cs_id, maybe_version, _maybe_source_repo)| {
                let version = maybe_version.ok_or_else(||
                    anyhow!(
                        "no sync commit version specified for remapping of {} -> {} (source repo {}, target repo {})",
                        source_cs_id.0, cs_id,
                        source_repo_id,
                        target_repo_id,
                    )
                )?;

                Ok((cs_id, version))
            })
            .collect();
        let remapped = remapped?;
        return Ok(Some(PluralCommitSyncOutcome::RewrittenAs(remapped)));
    }

    let maybe_wc_equivalence = mapping
        .get_equivalent_working_copy(ctx, source_repo_id.0, source_cs_id.0, target_repo_id.0)
        .await?;

    match maybe_wc_equivalence {
        None => {
            if direction == CommitSyncDirection::Backwards {
                let maybe_version = mapping
                    .get_large_repo_commit_version(ctx, source_repo_id.0, source_cs_id.0)
                    .await?;

                if let Some(version) = maybe_version {
                    let small_repos = get_small_repos_for_version(
                        live_commit_sync_config,
                        source_repo_id.0,
                        &version,
                    )
                    .await?;
                    if !small_repos.contains(&target_repo_id.0) {
                        return Ok(Some(PluralCommitSyncOutcome::NotSyncCandidate(version)));
                    }
                }
                Ok(None)
            } else {
                Ok(None)
            }
        }
        Some(WorkingCopyEquivalence::NoWorkingCopy(version)) => {
            Ok(Some(PluralCommitSyncOutcome::NotSyncCandidate(version)))
        }
        Some(WorkingCopyEquivalence::WorkingCopy(cs_id, version)) => Ok(Some(
            PluralCommitSyncOutcome::EquivalentWorkingCopyAncestor(cs_id, version),
        )),
    }
}

/// Check if commit has been synced (or at least considered to be synced)
/// between repos
/// The confusing sentence above means that existing
/// `EquivalentWorkingCopyAncestor` or `NotSyncCandidate` outcomes
/// cause this fn to return true
pub async fn commit_sync_outcome_exists<'a, M: SyncedCommitMapping>(
    ctx: &'a CoreContext,
    source_repo_id: Source<RepositoryId>,
    target_repo_id: Target<RepositoryId>,
    source_cs_id: Source<ChangesetId>,
    mapping: &'a M,
    direction: CommitSyncDirection,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<bool, Error> {
    Ok(get_plural_commit_sync_outcome(
        ctx,
        source_repo_id,
        target_repo_id,
        source_cs_id,
        mapping,
        direction,
        live_commit_sync_config,
    )
    .await?
    .is_some())
}

/// Get `CommitSyncOutcome` for `source_cs_id`
/// This function fails if `source_cs_id` has been rewritten
/// into multiple different commits in the target repo.
pub async fn get_commit_sync_outcome<'a, M: SyncedCommitMapping>(
    ctx: &'a CoreContext,
    source_repo_id: Source<RepositoryId>,
    target_repo_id: Target<RepositoryId>,
    source_cs_id: Source<ChangesetId>,
    mapping: &'a M,
    direction: CommitSyncDirection,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<Option<CommitSyncOutcome>, Error> {
    get_commit_sync_outcome_with_hint(
        ctx,
        source_repo_id,
        target_repo_id,
        source_cs_id,
        mapping,
        CandidateSelectionHint::<!>::Only,
        direction,
        live_commit_sync_config,
    )
    .await
}

/// Get `CommitSyncOutcome` for `source_cs_id`
/// If `source_cs_id` is remapped into just one commit in the target
/// repo, this function works the same way as `get_commit_sync_outcome`
/// If `source_cs_id` is remapped as multiple commits in the target repo,
/// this function will use the `hint` to try to figure out which one to
/// select. The `hint` allows the user of this function to express the
/// desired topological relationship between `source_cs_id`'s selected
/// remapping and some other changeset or a bookmark. For example,
/// the user of this function can request that an ancestor of some
/// bookmark is selected from multiple `source_cs_id` remappings.
/// Important: if there's just one remapping, this function will
/// always select it, even if it does not satisfy the desired relationship.
pub async fn get_commit_sync_outcome_with_hint<'a, M: SyncedCommitMapping, R: Repo>(
    ctx: &'a CoreContext,
    source_repo_id: Source<RepositoryId>,
    target_repo_id: Target<RepositoryId>,
    source_cs_id: Source<ChangesetId>,
    mapping: &'a M,
    hint: CandidateSelectionHint<R>,
    direction: CommitSyncDirection,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<Option<CommitSyncOutcome>, Error> {
    let maybe_plural_commit_sync_outcome = get_plural_commit_sync_outcome(
        ctx,
        source_repo_id,
        target_repo_id,
        source_cs_id,
        mapping,
        direction,
        live_commit_sync_config,
    )
    .await?;
    debug!(
        ctx.logger(),
        "get_commit_sync_outcome_with_hint called for {}->{}, cs {}, hint {:?}",
        source_repo_id.0,
        target_repo_id.0,
        source_cs_id.0,
        hint
    );
    let maybe_commit_sync_outcome = match maybe_plural_commit_sync_outcome {
        Some(plural_commit_sync_outcome) => match hint.try_into_desired_relationship(ctx).await? {
            None => Some(
                plural_commit_sync_outcome
                    .try_into_commit_sync_outcome(source_cs_id)
                    .await?,
            ),
            Some(desired_relationship) => {
                debug!(
                    ctx.logger(),
                    "CandidateSelectionHint converted into: {:?}", desired_relationship
                );
                Some(
                    plural_commit_sync_outcome
                        .try_into_commit_sync_outcome_with_desired_relationship(
                            ctx,
                            source_cs_id,
                            target_repo_id,
                            desired_relationship,
                        )
                        .await?,
                )
            }
        },
        None => None,
    };

    Ok(maybe_commit_sync_outcome)
}

trait SelectedCandidateFuture =
    Future<Output = Result<(ChangesetId, CommitSyncConfigVersion), Error>>;

/// An async fn to return one out of many `(cs_id, maybe_version)` candidates
trait CandidateSelector<'a> = FnOnce(
    Vec<(ChangesetId, CommitSyncConfigVersion)>,
) -> Pin<Box<dyn SelectedCandidateFuture + 'a + Send>>;

/// Get a `CandidateSelector` which either produces the only candidate item
/// or errors out
fn get_only_item_selector<'a>(
    original_source_cs_id: Source<ChangesetId>,
) -> impl CandidateSelector<'a> {
    let inner = move |v: Vec<(ChangesetId, CommitSyncConfigVersion)>| async move {
        let mut v = v.into_iter();
        match (v.next(), v.next()) {
            (None, None) => Err(anyhow!(
                "ProgrammingError: PluralCommitSyncOutcome::RewrittenAs has 0-sized payload for {}",
                original_source_cs_id
            )),
            (Some((cs_id, version)), None) => Ok((cs_id, version)),
            (Some((first, _)), Some((second, _))) => Err(anyhow!(
                "Too many rewritten candidates for {}: {}, {} (may be more)",
                original_source_cs_id,
                first,
                second
            )),
            (None, Some(_)) => panic!("iterator cannot produce Some after None"),
        }
    };

    move |v| {
        let r: Pin<Box<dyn SelectedCandidateFuture + Send + 'a>> = Box::pin(inner(v));
        r
    }
}

/// Desired topological relationship to look for
/// while iterating over the list of candidate changesets
/// This struct is a simplified version of `CandidateSelectionHint`:
/// - it does not deal with bookmarks
/// - it does not deal with the expectation of having only one candidate in the list
pub enum DesiredRelationship<R: Repo> {
    /// Changeset should be an ancestor of this variant's payload
    /// Note: in this case any changeset is an ancestor of itself
    AncestorOf(Target<ChangesetId>, Target<R>),
    /// Changeset should be a descendant of this variant's payload
    /// Note: in this case any changeset is a descendant of itself
    DescendantOf(Target<ChangesetId>, Target<R>),
    /// Changeset should the same as this variant's payload
    EqualTo(Target<ChangesetId>),
}

impl<R: Repo> fmt::Debug for DesiredRelationship<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AncestorOf(cs_id, _) => {
                write!(f, "DesiredRelationship::AncestorOf({})", cs_id.0)
            }
            Self::DescendantOf(cs_id, _) => {
                write!(f, "DesiredRelationship::DescendantOf({})", cs_id.0)
            }
            Self::EqualTo(cs_id) => write!(f, "DesiredRelationship::EqualTo({})", cs_id.0),
        }
    }
}

impl<R: Repo> DesiredRelationship<R> {
    /// Get the payload changeset of the desired relationship
    fn cs_id(&self) -> Target<ChangesetId> {
        match self {
            Self::EqualTo(cs_id) => *cs_id,
            Self::AncestorOf(cs_id, _) => *cs_id,
            Self::DescendantOf(cs_id, _) => *cs_id,
        }
    }

    /// Get an error message for a case when `DesiredRelationship`
    /// narrowed things down too much
    fn none_err_msg(
        &self,
        original_source_cs_id: Source<ChangesetId>,
        target_repo_id: Target<RepositoryId>,
    ) -> String {
        match self {
            Self::AncestorOf(cs_id, _) => format!(
                "{} does not rewrite into any ancestor of {} in {}",
                original_source_cs_id, cs_id.0, target_repo_id.0
            ),
            Self::DescendantOf(cs_id, _) => format!(
                "{} does not rewrite into any descendant of {} in {}",
                original_source_cs_id, cs_id.0, target_repo_id.0
            ),
            Self::EqualTo(cs_id) => format!(
                "{} does not rewrite into {} in {}",
                original_source_cs_id, cs_id.0, target_repo_id.0
            ),
        }
    }

    /// Get an error message for a case when `DesiredRelationship` did not
    /// narrow things down enough
    fn multiple_err_msg(
        &self,
        original_source_cs_id: Source<ChangesetId>,
        target_cs_id_1: ChangesetId,
        target_cs_id_2: ChangesetId,
        target_repo_id: Target<RepositoryId>,
    ) -> String {
        match self {
            Self::AncestorOf(cs_id, _) => format!(
                "{} rewrites into multiple ancestors of {} in {}: {}, {} (may be more)",
                original_source_cs_id, cs_id.0, target_repo_id.0, target_cs_id_1, target_cs_id_2
            ),
            Self::DescendantOf(cs_id, _) => format!(
                "{} rewrites into multiple descendants of {} in {}: {}, {} (may be more)",
                original_source_cs_id, cs_id.0, target_repo_id.0, target_cs_id_1, target_cs_id_2
            ),
            // Nonsense case: two separate rewritings into the same commit
            // Let's still error out to fail the request, but not crash process.
            Self::EqualTo(cs_id) => format!(
                "Should be impossible. {} rewrites into {} and {}, both equal to {} in {}",
                original_source_cs_id, target_cs_id_1, target_cs_id_2, cs_id.0, target_repo_id.0
            ),
        }
    }

    /// Check if a `target_cs_id` is in this relationship
    async fn holds_for<'a>(
        &'a self,
        ctx: &'a CoreContext,
        target_cs_id: Target<ChangesetId>,
    ) -> Result<bool, Error> {
        if target_cs_id == self.cs_id() {
            return Ok(true);
        }

        match self {
            Self::EqualTo(expected_cs_id) => Ok(target_cs_id == *expected_cs_id),
            Self::AncestorOf(comparison_cs_id, target_repo) => {
                target_repo
                    .commit_graph()
                    .is_ancestor(ctx, target_cs_id.0, comparison_cs_id.0)
                    .await
            }
            Self::DescendantOf(comparison_cs_id, target_repo) => {
                target_repo
                    .commit_graph()
                    .is_ancestor(ctx, comparison_cs_id.0, target_cs_id.0)
                    .await
            }
        }
    }
}

/// Get a `CandidateSelector` which produces:
/// - the only candidate
/// - or if there are multiple, the only one in the desired topological relationship
fn get_only_or_in_desired_relationship_selector<'a, R: Repo>(
    ctx: &'a CoreContext,
    original_source_cs_id: Source<ChangesetId>,
    target_repo_id: Target<RepositoryId>,
    desired_relationship: DesiredRelationship<R>,
) -> impl CandidateSelector<'a> {
    let inner = move |v: Vec<(ChangesetId, CommitSyncConfigVersion)>| async move {
        if v.len() == 1 {
            let first = v.into_iter().next().unwrap();
            return Ok(first);
        }

        // A list of candidate items, which are in correct relationship
        let candidates: Vec<Option<(ChangesetId, CommitSyncConfigVersion)>> =
            try_join_all(v.into_iter().map(|(cs_id, maybe_version)| {
                let desired_relationship = &desired_relationship;
                async move {
                    if desired_relationship.holds_for(ctx, Target(cs_id)).await? {
                        Result::<_, Error>::Ok(Some((cs_id, maybe_version)))
                    } else {
                        Result::<_, Error>::Ok(None)
                    }
                }
            }))
            .await?;

        let mut candidates = candidates.into_iter().flatten();
        match (candidates.next(), candidates.next()) {
            (None, None) => Err(anyhow!(
                "{}",
                desired_relationship.none_err_msg(original_source_cs_id, target_repo_id)
            )),
            (Some((cs_id, maybe_version)), None) => Ok((cs_id, maybe_version)),
            (Some((first, _)), Some((second, _))) => Err(anyhow!(
                "{}",
                desired_relationship.multiple_err_msg(
                    original_source_cs_id,
                    first,
                    second,
                    target_repo_id
                )
            )),
            (None, Some(_)) => panic!("iterator cannot produce Some after None"),
        }
    };

    move |v| {
        let r: Pin<Box<dyn SelectedCandidateFuture + Send + 'a>> = Box::pin(inner(v));
        r
    }
}

impl PluralCommitSyncOutcome {
    /// Consume `self` and produce singular `CommitSyncOutcome`
    /// using a specified `CandidateSelector`
    async fn try_into_commit_sync_outcome_with_selector<'a>(
        self,
        selector: impl CandidateSelector<'a>,
    ) -> Result<CommitSyncOutcome, Error> {
        use PluralCommitSyncOutcome::*;
        match self {
            NotSyncCandidate(version) => Ok(CommitSyncOutcome::NotSyncCandidate(version)),
            EquivalentWorkingCopyAncestor(cs_id, version) => Ok(
                CommitSyncOutcome::EquivalentWorkingCopyAncestor(cs_id, version),
            ),
            RewrittenAs(v) => {
                let (cs_id, version) = selector(v).await?;
                Ok(CommitSyncOutcome::RewrittenAs(cs_id, version))
            }
        }
    }

    /// Get `CommitSyncOutcome` out of `self`
    /// Error out if `self` is `RewrittenAs` and its payload
    /// has >1 item
    pub async fn try_into_commit_sync_outcome(
        self,
        original_source_cs_id: Source<ChangesetId>,
    ) -> Result<CommitSyncOutcome, Error> {
        let selector = get_only_item_selector(original_source_cs_id);
        self.try_into_commit_sync_outcome_with_selector(selector)
            .await
    }

    /// Filters the plural sync results to those conforming to desired relationship.
    pub async fn filter_by_desired_relationship<R: Repo>(
        self,
        ctx: &CoreContext,
        desired_relationship: &DesiredRelationship<R>,
    ) -> Result<Self, Error> {
        use PluralCommitSyncOutcome::*;
        Ok(match self {
            NotSyncCandidate(version) => Self::NotSyncCandidate(version),
            EquivalentWorkingCopyAncestor(cs_id, version) => {
                Self::EquivalentWorkingCopyAncestor(cs_id, version)
            }
            RewrittenAs(v) => RewrittenAs(
                try_join_all(v.into_iter().map(|(cs_id, version)| {
                    desired_relationship
                        .holds_for(ctx, Target(cs_id))
                        .map_ok(move |res| res.then_some((cs_id, version)))
                }))
                .await?
                .into_iter()
                .filter_map(|x| x)
                .collect(),
            ),
        })
    }

    async fn try_into_commit_sync_outcome_with_desired_relationship<R: Repo>(
        self,
        ctx: &CoreContext,
        original_source_cs_id: Source<ChangesetId>,
        target_repo_id: Target<RepositoryId>,
        desired_relationship: DesiredRelationship<R>,
    ) -> Result<CommitSyncOutcome, Error> {
        let selector = get_only_or_in_desired_relationship_selector(
            ctx,
            original_source_cs_id,
            target_repo_id,
            desired_relationship,
        );
        self.try_into_commit_sync_outcome_with_selector(selector)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bookmarks::BookmarkUpdateReason;
    use fbinit::FacebookInit;
    use live_commit_sync_config::TestLiveCommitSyncConfig;
    use mononoke_macros::mononoke;
    use mononoke_types_mocks::changesetid::FOURS_CSID;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use rendezvous::RendezVousOptions;
    use sql::rusqlite::Connection as SqliteConnection;
    use sql_construct::SqlConstruct;
    use sql_ext::Connection;
    use sql_ext::SqlConnections;
    use synced_commit_mapping::SqlSyncedCommitMapping;
    use synced_commit_mapping::SqlSyncedCommitMappingBuilder;
    use synced_commit_mapping::SyncedCommitMappingEntry;
    use synced_commit_mapping::SyncedCommitSourceRepo;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::drawdag::create_from_dag;

    use super::*;
    use crate::test_utils::TestRepo;

    const SMALL_REPO_ID: RepositoryId = RepositoryId::new(0);
    const LARGE_REPO_ID: RepositoryId = RepositoryId::new(1);

    fn test_version() -> CommitSyncConfigVersion {
        CommitSyncConfigVersion("test_version".to_string())
    }

    /// Get a new instance of mapping with `entries` inserted
    /// (left: small cs, right: large cs)
    async fn get_new_mapping(
        ctx: &CoreContext,
        entries: Vec<(ChangesetId, ChangesetId)>,
        small_repo_id: RepositoryId,
        large_repo_id: RepositoryId,
    ) -> Result<SqlSyncedCommitMapping, Error> {
        let sqlite_con = SqliteConnection::open_in_memory()?;
        sqlite_con.execute_batch(SqlSyncedCommitMappingBuilder::CREATION_QUERY)?;
        let con = Connection::with_sqlite(sqlite_con)?;
        let m =
            SqlSyncedCommitMappingBuilder::from_sql_connections(SqlConnections::new_single(con))
                .build(RendezVousOptions::for_test());
        for (small_bcs_id, large_bcs_id) in entries {
            m.add(
                ctx,
                SyncedCommitMappingEntry::new(
                    large_repo_id,
                    large_bcs_id,
                    small_repo_id,
                    small_bcs_id,
                    test_version(),
                    SyncedCommitSourceRepo::Small,
                ),
            )
            .await?;
        }
        Ok(m)
    }

    async fn get_selection_result(
        ctx: &CoreContext,
        candidates: Vec<ChangesetId>,
        hint: CandidateSelectionHint<TestRepo>,
    ) -> Result<Option<CommitSyncOutcome>, Error> {
        let entries: Vec<_> = candidates
            .iter()
            .map(|large_cs_id| (ONES_CSID, *large_cs_id))
            .collect();
        let mapping = get_new_mapping(ctx, entries, SMALL_REPO_ID, LARGE_REPO_ID).await?;
        let live_commit_sync_config = Arc::new(TestLiveCommitSyncConfig::new_empty());

        get_commit_sync_outcome_with_hint(
            ctx,
            Source(SMALL_REPO_ID),
            Target(LARGE_REPO_ID),
            Source(ONES_CSID),
            &mapping,
            hint,
            CommitSyncDirection::Forward,
            live_commit_sync_config,
        )
        .await
    }

    async fn verify_selection_success(
        ctx: &CoreContext,
        candidates: Vec<ChangesetId>,
        expected_selected_candidate: ChangesetId,
        hint: CandidateSelectionHint<TestRepo>,
    ) -> Result<(), Error> {
        let outcome = get_selection_result(ctx, candidates, hint).await?;
        assert_eq!(
            outcome,
            Some(CommitSyncOutcome::RewrittenAs(
                expected_selected_candidate,
                test_version(),
            ))
        );
        Ok(())
    }

    async fn verify_selection_failure(
        ctx: &CoreContext,
        candidates: Vec<ChangesetId>,
        expected_error_message: &str,
        hint: CandidateSelectionHint<TestRepo>,
    ) -> Result<(), Error> {
        let selection_error = get_selection_result(ctx, candidates, hint)
            .await
            .expect_err("selection was expected to fail");

        assert!(format!("{:?}", selection_error).contains(expected_error_message));
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_ancestor_hint_selector(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo: TestRepo = TestRepoFactory::new(fb)?
            .with_id(LARGE_REPO_ID)
            .build()
            .await?;
        let dag = create_from_dag(
            &ctx,
            &blob_repo,
            r"
                A-B-C-D-E
                   \
                    F-G
            ",
        )
        .await?;

        let c = *dag.get("C").unwrap();
        let f = *dag.get("F").unwrap();
        let e = *dag.get("E").unwrap();
        let g = *dag.get("G").unwrap();

        use CandidateSelectionHint::*;
        // candidates on different branches, one is in the desired relationship
        verify_selection_success(
            &ctx,
            vec![c, f],
            c,
            AncestorOfCommit(Target(e), Target(blob_repo.clone())),
        )
        .await?;

        // cnadidates on different branches, one is in the desired relationship
        // (the one in the second place in the candidate list)
        verify_selection_success(
            &ctx,
            vec![c, f],
            f,
            AncestorOfCommit(Target(g), Target(blob_repo.clone())),
        )
        .await?;

        // None of the candidates is a proper ancestor of `c`,
        // but one of the candidates is `c` itself
        verify_selection_success(
            &ctx,
            vec![c, f],
            c,
            AncestorOfCommit(Target(c), Target(blob_repo.clone())),
        )
        .await?;

        // None of the candidates is an ancestor of the desired descendant,
        // but there's just 1 candidate in a list
        verify_selection_success(
            &ctx,
            vec![c],
            c,
            AncestorOfCommit(Target(f), Target(blob_repo.clone())),
        )
        .await?;

        // No ancestor and multiple elements on the list, should fail
        verify_selection_failure(
            &ctx,
            vec![c, e],
            "does not rewrite into any ancestor of",
            AncestorOfCommit(Target(g), Target(blob_repo.clone())),
        )
        .await?;

        // Multiple ancestors on the list, should fail
        verify_selection_failure(
            &ctx,
            vec![c, e],
            "rewrites into multiple ancestors of",
            AncestorOfCommit(Target(e), Target(blob_repo.clone())),
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_descendant_hint_selector(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let dag = create_from_dag(
            &ctx,
            &blob_repo,
            r"
                A-B----C--D-E
                   \    \
                    F-G  J
            ",
        )
        .await?;

        let b = *dag.get("B").unwrap();
        let f = *dag.get("F").unwrap();
        let e = *dag.get("E").unwrap();
        let g = *dag.get("G").unwrap();
        let j = *dag.get("J").unwrap();
        let d = *dag.get("D").unwrap();

        use CandidateSelectionHint::*;

        // Candidates on different branches, one of them is a descendant
        // of the desired ancestor
        verify_selection_success(
            &ctx,
            vec![e, j],
            e,
            DescendantOfCommit(Target(d), Target(blob_repo.clone())),
        )
        .await?;

        // Candidates on different branches, one of them is a descendant
        // of the desired ancestor (not the first one in the list)
        verify_selection_success(
            &ctx,
            vec![e, g],
            g,
            DescendantOfCommit(Target(f), Target(blob_repo.clone())),
        )
        .await?;

        // Candidates on different branches, one of them is the desired ancestor itself
        verify_selection_success(
            &ctx,
            vec![e, g],
            g,
            DescendantOfCommit(Target(g), Target(blob_repo.clone())),
        )
        .await?;

        // Only one candidate, which is not a descendant of a desired ancestor,
        // but is successfully selected nevertheless as a the only option
        verify_selection_success(
            &ctx,
            vec![e],
            e,
            DescendantOfCommit(Target(g), Target(blob_repo.clone())),
        )
        .await?;

        // None of the candidates is the descendant of the desired ancestor
        verify_selection_failure(
            &ctx,
            vec![e, j],
            "does not rewrite into any descendant of",
            DescendantOfCommit(Target(f), Target(blob_repo.clone())),
        )
        .await?;

        // Both candidates are descendants of the desired ancestor
        verify_selection_failure(
            &ctx,
            vec![e, d],
            "rewrites into multiple descendants of",
            DescendantOfCommit(Target(b), Target(blob_repo.clone())),
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_exact_hint_selector(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        use CandidateSelectionHint::*;

        // there's just one candidate and it's equal to the expected value
        verify_selection_success(&ctx, vec![TWOS_CSID], TWOS_CSID, Exact(Target(TWOS_CSID)))
            .await?;

        // expected value is among the candidates
        verify_selection_success(
            &ctx,
            vec![THREES_CSID, TWOS_CSID],
            TWOS_CSID,
            Exact(Target(TWOS_CSID)),
        )
        .await?;

        // expected value is not among the candidates
        verify_selection_failure(
            &ctx,
            vec![FOURS_CSID, TWOS_CSID],
            "does not rewrite into",
            Exact(Target(THREES_CSID)),
        )
        .await?;

        Ok(())
    }

    async fn set_bookmark(
        ctx: &CoreContext,
        blob_repo: &TestRepo,
        bcs_id: ChangesetId,
        book: &BookmarkKey,
    ) -> Result<(), Error> {
        let mut txn = blob_repo.bookmarks().create_transaction(ctx.clone());
        txn.force_set(book, bcs_id, BookmarkUpdateReason::TestMove)
            .unwrap();
        txn.commit().await?;
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_bookmark_hint_selector(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo: TestRepo = TestRepoFactory::new(fb)?
            .with_id(LARGE_REPO_ID)
            .build()
            .await?;
        let dag = create_from_dag(
            &ctx,
            &blob_repo,
            r"
                A-B-C-D-E
                   \
                    F-G
            ",
        )
        .await?;

        let c = *dag.get("C").unwrap();
        let f = *dag.get("F").unwrap();
        let e = *dag.get("E").unwrap();
        let g = *dag.get("G").unwrap();

        let book_e = BookmarkKey::new("book_e").unwrap();
        set_bookmark(&ctx, &blob_repo, e, &book_e).await?;
        let book_g = BookmarkKey::new("book_g").unwrap();
        set_bookmark(&ctx, &blob_repo, g, &book_g).await?;

        use CandidateSelectionHint::*;
        // candidates on different branches, one is in the desired relationship with a bookmark
        verify_selection_success(
            &ctx,
            vec![c, f],
            c,
            AncestorOfBookmark(Target(book_e.clone()), Target(blob_repo.clone())),
        )
        .await?;

        // When bokmark does not exist, we fall back to `Only` rather than fail
        verify_selection_success(
            &ctx,
            vec![f],
            f,
            AncestorOfBookmark(Target(book_g.clone()), Target(blob_repo.clone())),
        )
        .await?;

        // No ancestor and multiple elements on the list, should fail
        verify_selection_failure(
            &ctx,
            vec![f, g],
            "does not rewrite into any ancestor of",
            AncestorOfBookmark(Target(book_e), Target(blob_repo.clone())),
        )
        .await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_only_hint(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo: TestRepo = TestRepoFactory::new(fb)?
            .with_id(LARGE_REPO_ID)
            .build()
            .await?;
        let dag = create_from_dag(
            &ctx,
            &blob_repo,
            r"
                A-B-C-D-E
                   \
                    F-G
            ",
        )
        .await?;

        let c = *dag.get("C").unwrap();
        let f = *dag.get("F").unwrap();

        use CandidateSelectionHint::Only;

        verify_selection_success(&ctx, vec![c], c, Only).await?;
        verify_selection_failure(&ctx, vec![c, f], "Too many rewritten candidates", Only).await?;

        Ok(())
    }
}
