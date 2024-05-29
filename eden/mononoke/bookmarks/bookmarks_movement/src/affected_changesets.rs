/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use context::CoreContext;
use cross_repo_sync::CHANGE_XREPO_MAPPING_EXTRA;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::FbStreamExt;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use hooks::PushAuthoredBy;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;
use skeleton_manifest::RootSkeletonManifestId;

use crate::hook_running::run_hooks;
use crate::restrictions::should_run_hooks;
use crate::BookmarkMovementError;
use crate::Repo;

const N_CHANGESETS_TO_LOAD_AT_ONCE: usize = 1000;
const DEFAULT_ADDITIONAL_CHANGESETS_LIMIT: usize = 200000;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum AdditionalChangesets {
    None,
    Ancestors(ChangesetId),
    Range {
        head: ChangesetId,
        base: ChangesetId,
    },
}

pub(crate) struct AffectedChangesets {
    /// Changesets that are being added to the repository and to this bookmark.
    new_changesets: HashMap<ChangesetId, BonsaiChangeset>,

    /// Changesets that are being used as a source for pushrebase.
    source_changesets: HashSet<BonsaiChangeset>,

    /// Max limit on how many additional changesets to load
    additional_changesets_limit: usize,

    /// Changesets that we have already checked.
    /// This could be a large number, but we only store hashes.
    /// This avoids performing the same checks twice, but more importantly, reloading the same
    /// changesets over and over again in the case of additional changesets.
    already_checked_changesets: HashSet<ChangesetId>,
}

impl AffectedChangesets {
    pub(crate) fn with_limit(limit: Option<usize>) -> Self {
        Self {
            new_changesets: HashMap::new(),
            source_changesets: HashSet::new(),
            additional_changesets_limit: limit.unwrap_or(DEFAULT_ADDITIONAL_CHANGESETS_LIMIT),
            already_checked_changesets: HashSet::new(),
        }
    }

    pub(crate) fn with_source_changesets(source_changesets: HashSet<BonsaiChangeset>) -> Self {
        Self {
            new_changesets: HashMap::new(),
            source_changesets,
            additional_changesets_limit: DEFAULT_ADDITIONAL_CHANGESETS_LIMIT,
            already_checked_changesets: HashSet::new(),
        }
    }

    pub(crate) fn add_new_changesets(
        &mut self,
        new_changesets: HashMap<ChangesetId, BonsaiChangeset>,
    ) {
        if self.new_changesets.is_empty() {
            self.new_changesets = new_changesets;
        } else {
            self.new_changesets.extend(new_changesets);
        }
    }

    pub(crate) fn new_changesets(&self) -> &HashMap<ChangesetId, BonsaiChangeset> {
        &self.new_changesets
    }

    pub(crate) fn source_changesets(&self) -> &HashSet<BonsaiChangeset> {
        &self.source_changesets
    }

    fn adding_new_changesets_to_repo(&self) -> bool {
        !self.source_changesets.is_empty() || !self.new_changesets.is_empty()
    }

    /// Load bonsais in the additional changeset range that are not already in
    /// `new_changesets` and are ancestors of `head` but not ancestors of `base`
    /// or any of the `hooks_skip_ancestors_of` bookmarks for the named
    /// bookmark.
    ///
    /// These are the additional bonsais that we need to run hooks on for
    /// bookmark moves.
    async fn load_additional_changesets<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bookmark: &BookmarkKey,
        additional_changesets: AdditionalChangesets,
    ) -> Result<BoxStream<'a, Result<BonsaiChangeset, BookmarkMovementError>>> {
        let (head, base) = match additional_changesets {
            AdditionalChangesets::None => {
                return Ok(stream::empty().boxed());
            }
            AdditionalChangesets::Ancestors(head) => (head, None),
            AdditionalChangesets::Range { head, base } => (head, Some(base)),
        };

        let mut exclude_bookmarks: HashSet<_> = repo
            .repo_bookmark_attrs()
            .select(bookmark)
            .flat_map(|attr| attr.params().hooks_skip_ancestors_of.iter())
            .cloned()
            .collect();
        exclude_bookmarks.remove(bookmark);

        let mut excludes: HashSet<_> = stream::iter(exclude_bookmarks)
            .map(|bookmark| repo.bookmarks().get(ctx.clone(), &bookmark))
            .buffered(100)
            .try_filter_map(|maybe_cs_id| async move { Ok(maybe_cs_id) })
            .try_collect()
            .await?;
        excludes.extend(base);

        let range = repo
            .commit_graph()
            .ancestors_difference_stream(ctx, vec![head], excludes.into_iter().collect())
            .await?
            .yield_periodically()
            .try_filter(|bcs_id| {
                let exists = self.new_changesets.contains_key(bcs_id);
                let already_checked = self.already_checked_changesets.contains(bcs_id);
                future::ready(!exists && !already_checked)
            });

        let additional_changesets_limit = self.additional_changesets_limit;

        let additional_changesets = if justknobs::eval(
            "scm/mononoke:run_hooks_on_additional_changesets",
            None,
            None,
        )
        .unwrap_or(true)
        {
            let bonsais = range
                .and_then({
                    let mut count = 0;
                    move |bcs_id| {
                        count += 1;
                        if count > additional_changesets_limit {
                            future::ready(Err(anyhow!(
                                "bookmark movement additional changesets limit reached at {}",
                                bcs_id
                            )))
                        } else {
                            future::ready(Ok(bcs_id))
                        }
                    }
                })
                .map(move |res| async move {
                    match res {
                        Ok(bcs_id) => Ok(bcs_id
                            .load(ctx, repo.repo_blobstore())
                            .await
                            .map_err(|e| BookmarkMovementError::Error(e.into()))?),
                        Err(e) => Err(e.into()),
                    }
                })
                .buffered(N_CHANGESETS_TO_LOAD_AT_ONCE)
                .boxed();

            ctx.scuba()
                .clone()
                .add("hook_running_additional_changesets", None::<usize>)
                .log_with_msg("Running hooks for additional changesets", None);
            bonsais
        } else {
            // Logging-only mode.  Work out how many changesets we would have run
            // on, and whether the limit would have been reached.
            let count = range
                .take(additional_changesets_limit)
                .try_fold(0usize, |acc, _| async move { Ok(acc + 1) })
                .await?;

            let mut scuba = ctx.scuba().clone();
            scuba.add("hook_running_additional_changesets", count);
            if count >= additional_changesets_limit {
                scuba.add("hook_running_additional_changesets_limit_reached", true);
            }
            scuba.log_with_msg("Hook running skipping additional changesets", None);
            stream::empty().boxed()
        };

        Ok(additional_changesets)
    }

    async fn changesets_stream<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bookmark: &BookmarkKey,
        additional_changesets: AdditionalChangesets,
    ) -> Result<BoxStream<'a, Result<BonsaiChangeset, BookmarkMovementError>>> {
        let additional_changesets = self
            .load_additional_changesets(ctx, repo, bookmark, additional_changesets)
            .await?;
        Ok(stream::iter(
            self.new_changesets
                .values()
                .chain(self.source_changesets.iter())
                .filter_map(|r| {
                    if !self
                        .already_checked_changesets
                        .contains(&r.get_changeset_id())
                    {
                        Some(Ok(r.clone()))
                    } else {
                        None
                    }
                }),
        )
        .chain(additional_changesets)
        .boxed())
    }

    /// Check all applicable restrictions on the affected changesets.
    pub(crate) async fn check_restrictions(
        &mut self,
        ctx: &CoreContext,
        authz: &AuthorizationContext,
        repo: &impl Repo,
        hook_manager: &HookManager,
        bookmark: &BookmarkKey,
        pushvars: Option<&HashMap<String, Bytes>>,
        reason: BookmarkUpdateReason,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        let needs_extras_check = Self::needs_extras_check(repo, kind);
        let needs_case_conflicts_check = Self::needs_case_conflicts_check(repo, kind);
        let needs_hooks_check =
            Self::needs_hooks_check(kind, authz, reason, hook_manager, bookmark);
        let needs_path_permissions_check =
            Self::needs_path_permissions_check(ctx, authz, repo).await;
        let changesets_stream = if needs_extras_check
            || needs_case_conflicts_check
            || needs_hooks_check
            || needs_path_permissions_check
        {
            self.changesets_stream(ctx, repo, bookmark, additional_changesets)
                .await
                .context("Failed to load additional affected changesets to check restrictions")?
        } else {
            stream::empty().boxed()
        };

        self.already_checked_changesets = changesets_stream
            .chunks(N_CHANGESETS_TO_LOAD_AT_ONCE)
            // Aggregate any error loading changesets on a per-chunk basis
            .map(|chunk| chunk.into_iter().collect::<Result<Vec<_>, _>>())
            .try_fold(HashSet::new(), |mut checked_changesets, chunk| {
                let adding_new_changesets_to_repo = self.adding_new_changesets_to_repo();
                async move {
                    if needs_extras_check {
                        Self::check_extras(&chunk).await?;
                    }

                    if needs_case_conflicts_check {
                        Self::check_case_conflicts(&chunk, ctx, repo).await?;
                    }

                    if needs_hooks_check {
                        Self::check_hooks(
                            adding_new_changesets_to_repo,
                            &chunk,
                            ctx,
                            authz,
                            repo,
                            hook_manager,
                            bookmark,
                            pushvars,
                            cross_repo_push_source,
                        )
                        .await?;
                    }

                    if needs_path_permissions_check {
                        Self::check_path_permissions(&chunk, ctx, authz, repo).await?;
                    }
                    // No check failed. We can record this chunk of changesets as having passed the
                    // checks and avoid loading them again next time.
                    checked_changesets.extend(
                        chunk
                            .iter()
                            .map(|bcs| bcs.get_changeset_id())
                            .collect::<Vec<_>>(),
                    );
                    Ok(checked_changesets)
                }
            })
            .await?;

        Ok(())
    }

    fn needs_extras_check(repo: &impl Repo, kind: BookmarkKind) -> bool {
        (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && !repo
                .repo_config()
                .pushrebase
                .allow_change_xrepo_mapping_extra
    }

    async fn check_extras(
        loaded_changesets: &[BonsaiChangeset],
    ) -> Result<(), BookmarkMovementError> {
        for bcs in loaded_changesets {
            if bcs
                .hg_extra()
                .any(|(name, _)| name == CHANGE_XREPO_MAPPING_EXTRA)
            {
                // This extra is used in backsyncer, and it changes how commit
                // is rewritten from a large repo to a small repo. This is dangerous
                // operation, and we don't allow landing a commit with this extra set.
                return Err(anyhow!(
                    "Disallowed extra {} is set on {}.",
                    CHANGE_XREPO_MAPPING_EXTRA,
                    bcs.get_changeset_id()
                )
                .into());
            }
        }

        Ok(())
    }

    fn needs_case_conflicts_check(repo: &impl Repo, kind: BookmarkKind) -> bool {
        let config = &repo.repo_config().pushrebase.flags;
        (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && config.casefolding_check
    }

    /// If the push is to a public bookmark, and the casefolding check is
    /// enabled, check that no affected changeset has case conflicts.
    async fn check_case_conflicts(
        loaded_changesets: &[BonsaiChangeset],
        ctx: &CoreContext,
        repo: &impl Repo,
    ) -> Result<(), BookmarkMovementError> {
        stream::iter(loaded_changesets.iter().map(Ok))
            .try_for_each_concurrent(100, |bcs| {
                async move {
                    let bcs_id = bcs.get_changeset_id();

                    let sk_mf = repo
                        .repo_derived_data()
                        .derive::<RootSkeletonManifestId>(ctx, bcs_id)
                        .await
                        .map_err(Error::from)?
                        .into_skeleton_manifest_id()
                        .load(ctx, repo.repo_blobstore())
                        .await
                        .map_err(Error::from)?;
                    if sk_mf.has_case_conflicts() {
                        // We only reject a commit if it introduces new case
                        // conflicts compared to its parents.
                        let parents = stream::iter(bcs.parents().map(|parent_bcs_id| async move {
                            repo.repo_derived_data()
                                .derive::<RootSkeletonManifestId>(ctx, parent_bcs_id)
                                .await
                                .map_err(Error::from)?
                                .into_skeleton_manifest_id()
                                .load(ctx, repo.repo_blobstore())
                                .await
                                .map_err(Error::from)
                        }))
                        .buffered(10)
                        .try_collect::<Vec<_>>()
                        .await?;
                        let config = &repo.repo_config().pushrebase.flags;

                        if let Some((path1, path2)) = sk_mf
                            .first_new_case_conflict(
                                ctx,
                                repo.repo_blobstore(),
                                parents,
                                &config.casefolding_check_excluded_paths,
                            )
                            .await?
                        {
                            return Err(BookmarkMovementError::CaseConflict {
                                changeset_id: bcs_id,
                                path1,
                                path2,
                            });
                        }
                    }
                    Ok(())
                }
            })
            .await?;
        Ok(())
    }

    fn needs_hooks_check(
        kind: BookmarkKind,
        authz: &AuthorizationContext,
        reason: BookmarkUpdateReason,
        hook_manager: &HookManager,
        bookmark: &BookmarkKey,
    ) -> bool {
        let mut needs_hooks_check = (kind == BookmarkKind::Publishing
            || kind == BookmarkKind::PullDefaultPublishing)
            && should_run_hooks(authz, reason);
        if reason == BookmarkUpdateReason::Push {
            let disable_fallback_to_master =
                justknobs::eval("scm/mononoke:disable_hooks_on_plain_push", None, None)
                    .unwrap_or_default();
            if disable_fallback_to_master {
                // Skip running hooks for this plain push.
                needs_hooks_check = false;
            }
        }
        needs_hooks_check = needs_hooks_check && hook_manager.hooks_exist_for_bookmark(bookmark);
        needs_hooks_check
    }

    /// If this is a user-initiated update to a public bookmark, run the
    /// hooks against the affected changesets. Also run hooks if it is a
    /// service-initiated pushrebase but hooks will run with taking this
    /// into account.
    async fn check_hooks(
        adding_new_changesets_to_repo: bool,
        loaded_changesets: &[BonsaiChangeset],
        ctx: &CoreContext,
        authz: &AuthorizationContext,
        repo: &impl Repo,
        hook_manager: &HookManager,
        bookmark: &BookmarkKey,
        pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        let skip_running_hooks_if_public: bool = repo
            .repo_bookmark_attrs()
            .select(bookmark)
            .map(|attr| attr.params().allow_move_to_public_commits_without_hooks)
            .any(|x| x);
        if skip_running_hooks_if_public && !adding_new_changesets_to_repo {
            // For some bookmarks we allow to skip running hooks if:
            // 1) this is just a bookmark move i.e. no new commits are added or pushrebased to the repo
            // 2) we are allowed to skip commits for a bookmark like that
            // 3) if all commits that are affectd by this bookmark move are public (which means
            //  we should have already ran hooks for these commits).

            let cs_ids = loaded_changesets
                .iter()
                .map(|bcs| bcs.get_changeset_id())
                .collect::<Vec<_>>();
            let public = repo
                .phases()
                .get_public(ctx, cs_ids.clone(), false /* ephemeral_derive */)
                .await?;
            if public == cs_ids.into_iter().collect::<HashSet<_>>() {
                return Ok(());
            }
        }

        if !loaded_changesets.is_empty() {
            let push_authored_by = if authz.is_service() {
                PushAuthoredBy::Service
            } else {
                PushAuthoredBy::User
            };
            run_hooks(
                ctx,
                hook_manager,
                bookmark,
                loaded_changesets.iter(),
                pushvars,
                cross_repo_push_source,
                push_authored_by,
            )
            .await?;
        }

        Ok(())
    }

    // User is not permitted to write to all paths, check if the paths
    // touched by the changesets are permitted.
    async fn needs_path_permissions_check(
        ctx: &CoreContext,
        authz: &AuthorizationContext,
        repo: &impl Repo,
    ) -> bool {
        authz.check_any_path_write(ctx, repo).await.is_denied()
    }

    /// Check whether the user has permissions to modify the paths that are
    /// modified by the changesets that are affected by the bookmark move.
    async fn check_path_permissions(
        loaded_changesets: &[BonsaiChangeset],
        ctx: &CoreContext,
        authz: &AuthorizationContext,
        repo: &impl Repo,
    ) -> Result<(), BookmarkMovementError> {
        for cs in loaded_changesets {
            authz.require_changeset_paths_write(ctx, repo, cs).await?;
        }
        Ok(())
    }
}
