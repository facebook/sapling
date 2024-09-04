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
use borrowed::borrowed;
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

use crate::hook_running::run_bookmark_hooks;
use crate::hook_running::run_changeset_hooks;
use crate::restrictions::should_run_hooks;
use crate::BookmarkMovementError;
use crate::Repo;

const N_CHANGESETS_TO_LOAD_AT_ONCE: usize = 1000;

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

    /// Changesets that we have already checked.
    /// This could be a large number, but we only store hashes.
    /// This avoids performing the same checks twice, but more importantly, reloading the same
    /// changesets over and over again in the case of additional changesets.
    already_checked_changesets: HashSet<ChangesetId>,

    /// Checks should not run on additional changesets
    should_bypass_checks_on_additional_changesets: bool,
}

impl AffectedChangesets {
    pub(crate) fn new() -> Self {
        Self {
            new_changesets: HashMap::new(),
            source_changesets: HashSet::new(),
            already_checked_changesets: HashSet::new(),
            should_bypass_checks_on_additional_changesets: false,
        }
    }

    pub(crate) fn with_source_changesets(source_changesets: HashSet<BonsaiChangeset>) -> Self {
        Self {
            new_changesets: HashMap::new(),
            source_changesets,
            already_checked_changesets: HashSet::new(),
            should_bypass_checks_on_additional_changesets: false,
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

    pub(crate) fn bypass_checks_on_additional_changesets(&mut self) {
        self.should_bypass_checks_on_additional_changesets = true;
    }

    fn adding_new_changesets_to_repo(&self) -> bool {
        !self.source_changesets.is_empty() || !self.new_changesets.is_empty()
    }

    /// Load bonsais in the additional changeset range that are not already in
    /// `new_changesets` and are ancestors of `head` but not ancestors of `base`
    /// or of any publishing bookmark.
    ///
    /// These are the additional bonsais that we need to run hooks on for
    /// bookmark moves.
    async fn load_additional_changesets<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        additional_changesets: AdditionalChangesets,
    ) -> Result<BoxStream<'a, Result<BonsaiChangeset, BookmarkMovementError>>> {
        let (head, base) = match additional_changesets {
            AdditionalChangesets::None => {
                return Ok(stream::empty().boxed());
            }
            AdditionalChangesets::Ancestors(head) => (head, None),
            AdditionalChangesets::Range { head, base } => (head, Some(base)),
        };
        let public_frontier = repo
            .commit_graph()
            .ancestors_frontier_with(ctx, vec![head], |csid| {
                borrowed!(ctx, repo);
                async move {
                    Ok(repo
                        .phases()
                        .get_cached_public(ctx, vec![csid])
                        .await?
                        .contains(&csid))
                }
            })
            .await?
            .into_iter()
            .chain(base.into_iter())
            .collect();

        Ok(repo
            .commit_graph()
            .ancestors_difference_stream(ctx, vec![head], public_frontier)
            .await?
            .yield_periodically()
            .try_filter(|bcs_id| {
                let exists = self.new_changesets.contains_key(bcs_id);
                future::ready(!exists)
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
            .boxed())
    }

    async fn changesets_stream<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        additional_changesets: AdditionalChangesets,
    ) -> Result<BoxStream<'a, Result<BonsaiChangeset, BookmarkMovementError>>> {
        let additional_changesets = if self.should_bypass_checks_on_additional_changesets {
            stream::empty().boxed()
        } else {
            self.load_additional_changesets(ctx, repo, additional_changesets)
                .await?
        };
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
            self.changesets_stream(ctx, repo, additional_changesets)
                .await
                .context("Failed to load additional affected changesets to check restrictions")?
        } else {
            stream::empty().boxed()
        };

        if needs_hooks_check {
            let head = match additional_changesets {
                AdditionalChangesets::None => {
                    // Bookmark deletion. Nothing to do.
                    None
                }
                AdditionalChangesets::Ancestors(head) => Some(head),
                AdditionalChangesets::Range { head, base: _ } => Some(head),
            };
            if let Some(head) = head {
                let head = head
                    .load(ctx, repo.repo_blobstore())
                    .await
                    .map_err(|e| BookmarkMovementError::Error(e.into()))?;
                Self::check_bookmark_hooks(
                    &head,
                    ctx,
                    authz,
                    hook_manager,
                    bookmark,
                    pushvars,
                    cross_repo_push_source,
                )
                .await?;
            }
        }

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
                        Self::check_changeset_hooks(
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
    /// hooks against the bookmark. Also run hooks if it is a
    /// service-initiated pushrebase but hooks will run with taking this
    /// into account.
    async fn check_bookmark_hooks(
        to: &BonsaiChangeset,
        ctx: &CoreContext,
        authz: &AuthorizationContext,
        hook_manager: &HookManager,
        bookmark: &BookmarkKey,
        pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        let push_authored_by = if authz.is_service() {
            PushAuthoredBy::Service
        } else {
            PushAuthoredBy::User
        };
        run_bookmark_hooks(
            ctx,
            hook_manager,
            bookmark,
            to,
            pushvars,
            cross_repo_push_source,
            push_authored_by,
        )
        .await?;

        Ok(())
    }

    /// If this is a user-initiated update to a public bookmark, run the
    /// hooks against the affected changesets. Also run hooks if it is a
    /// service-initiated pushrebase but hooks will run with taking this
    /// into account.
    async fn check_changeset_hooks(
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
            run_changeset_hooks(
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
