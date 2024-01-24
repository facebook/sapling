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

    /// Additional changesets, if they have been loaded.
    additional_changesets: Option<HashSet<BonsaiChangeset>>,
}

impl AffectedChangesets {
    pub(crate) fn new() -> Self {
        Self {
            new_changesets: HashMap::new(),
            source_changesets: HashSet::new(),
            additional_changesets: None,
        }
    }

    pub(crate) fn with_source_changesets(source_changesets: HashSet<BonsaiChangeset>) -> Self {
        Self {
            new_changesets: HashMap::new(),
            source_changesets,
            additional_changesets: None,
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
    async fn load_additional_changesets(
        &mut self,
        ctx: &CoreContext,
        repo: &impl Repo,
        bookmark: &BookmarkKey,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), Error> {
        if self.additional_changesets.is_some() {
            return Ok(());
        }

        let (head, base) = match additional_changesets {
            AdditionalChangesets::None => {
                self.additional_changesets = Some(HashSet::new());
                return Ok(());
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
                future::ready(!exists)
            });

        const ADDITIONAL_CHANGESETS_LIMIT: usize = 200000;

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
                        if count > ADDITIONAL_CHANGESETS_LIMIT {
                            future::ready(Err(anyhow!(
                                "bookmark movement additional changesets limit reached at {}",
                                bcs_id
                            )))
                        } else {
                            future::ready(Ok(bcs_id))
                        }
                    }
                })
                .map(|res| async move {
                    match res {
                        Ok(bcs_id) => Ok(bcs_id.load(ctx, repo.repo_blobstore()).await?),
                        Err(e) => Err(e),
                    }
                })
                .buffered(100)
                .try_collect::<HashSet<_>>()
                .await?;

            ctx.scuba()
                .clone()
                .add("hook_running_additional_changesets", bonsais.len())
                .log_with_msg("Running hooks for additional changesets", None);

            bonsais
        } else {
            // Logging-only mode.  Work out how many changesets we would have run
            // on, and whether the limit would have been reached.
            let count = range
                .take(ADDITIONAL_CHANGESETS_LIMIT)
                .try_fold(0usize, |acc, _| async move { Ok(acc + 1) })
                .await?;

            let mut scuba = ctx.scuba().clone();
            scuba.add("hook_running_additional_changesets", count);
            if count >= ADDITIONAL_CHANGESETS_LIMIT {
                scuba.add("hook_running_additional_changesets_limit_reached", true);
            }
            scuba.log_with_msg("Hook running skipping additional changesets", None);
            HashSet::new()
        };

        self.additional_changesets = Some(additional_changesets);
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.new_changesets.is_empty()
            && self.source_changesets.is_empty()
            && self
                .additional_changesets
                .as_ref()
                .map_or(true, HashSet::is_empty)
    }

    fn iter(&self) -> impl Iterator<Item = &BonsaiChangeset> + Clone {
        self.new_changesets
            .values()
            .chain(self.source_changesets.iter())
            .chain(self.additional_changesets.iter().flatten())
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
        self.check_extras(ctx, repo, bookmark, kind, additional_changesets)
            .await?;

        self.check_case_conflicts(ctx, repo, bookmark, kind, additional_changesets)
            .await?;

        self.check_hooks(
            ctx,
            authz,
            repo,
            hook_manager,
            bookmark,
            pushvars,
            reason,
            kind,
            additional_changesets,
            cross_repo_push_source,
        )
        .await?;

        self.check_path_permissions(ctx, authz, repo, bookmark, additional_changesets)
            .await?;

        Ok(())
    }

    async fn check_extras(
        &mut self,
        ctx: &CoreContext,
        repo: &impl Repo,
        bookmark: &BookmarkKey,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        if (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && !repo
                .repo_config()
                .pushrebase
                .allow_change_xrepo_mapping_extra
        {
            self.load_additional_changesets(ctx, repo, bookmark, additional_changesets)
                .await
                .context("Failed to load additional affected changesets to check extras")?;

            for bcs in self.iter() {
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
        }

        Ok(())
    }

    /// If the push is to a public bookmark, and the casefolding check is
    /// enabled, check that no affected changeset has case conflicts.
    async fn check_case_conflicts(
        &mut self,
        ctx: &CoreContext,
        repo: &impl Repo,
        bookmark: &BookmarkKey,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        let config = &repo.repo_config().pushrebase.flags;
        if (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && config.casefolding_check
        {
            self.load_additional_changesets(ctx, repo, bookmark, additional_changesets)
                .await
                .context("Failed to load additional affected changesets to check case conflicts")?;

            stream::iter(self.iter().map(Ok))
                .try_for_each_concurrent(100, |bcs| {
                    borrowed!(config);
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
                            let parents =
                                stream::iter(bcs.parents().map(|parent_bcs_id| async move {
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
        }
        Ok(())
    }

    /// If this is a user-initiated update to a public bookmark, run the
    /// hooks against the affected changesets. Also run hooks if it is a
    /// service-initiated pushrebase but hooks will run with taking this
    /// into account.
    async fn check_hooks(
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
        if (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && should_run_hooks(authz, reason)
        {
            if reason == BookmarkUpdateReason::Push {
                let disable_fallback_to_master =
                    justknobs::eval("scm/mononoke:disable_hooks_on_plain_push", None, None)
                        .unwrap_or_default();
                if disable_fallback_to_master {
                    // Skip running hooks for this plain push.
                    return Ok(());
                }
            }

            if hook_manager.hooks_exist_for_bookmark(bookmark) {
                self.load_additional_changesets(ctx, repo, bookmark, additional_changesets)
                    .await
                    .context("Failed to load additional affected changesets to check hooks")?;

                let skip_running_hooks_if_public: bool = repo
                    .repo_bookmark_attrs()
                    .select(bookmark)
                    .map(|attr| attr.params().allow_move_to_public_commits_without_hooks)
                    .any(|x| x);
                if skip_running_hooks_if_public && !self.adding_new_changesets_to_repo() {
                    // For some bookmarks we allow to skip running hooks if:
                    // 1) this is just a bookmark move i.e. no new commits are added or pushrebased to the repo
                    // 2) we are allowed to skip commits for a bookmark like that
                    // 3) if all commits that are affectd by this bookmark move are public (which means
                    //  we should have already ran hooks for these commits).

                    let cs_ids = self
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

                if !self.is_empty() {
                    let push_authored_by = if authz.is_service() {
                        PushAuthoredBy::Service
                    } else {
                        PushAuthoredBy::User
                    };
                    run_hooks(
                        ctx,
                        hook_manager,
                        bookmark,
                        self.iter(),
                        pushvars,
                        cross_repo_push_source,
                        push_authored_by,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// Check whether the user has permissions to modify the paths that are
    /// modified by the changesets that are affected by the bookmark move.
    async fn check_path_permissions(
        &mut self,
        ctx: &CoreContext,
        authz: &AuthorizationContext,
        repo: &impl Repo,
        bookmark: &BookmarkKey,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        // For optimization, first check if the user is permitted to modify
        // all paths.  In that case we don't need to find out which paths were
        // affected.
        if authz.check_any_path_write(ctx, repo).await.is_denied() {
            // User is not permitted to write to all paths, check if the paths
            // touched by the changesets are permitted.
            self.load_additional_changesets(ctx, repo, bookmark, additional_changesets)
                .await
                .context(
                    "Failed to load additional affected changesets to check path permissions",
                )?;

            for cs in self.iter() {
                authz.require_changeset_paths_write(ctx, repo, cs).await?;
            }
        }
        Ok(())
    }
}
