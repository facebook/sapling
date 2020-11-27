/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, Context, Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::stream::{self, StreamExt, TryStreamExt};
use hooks::{CrossRepoPushSource, HookManager};
use metaconfig_types::{BookmarkAttrs, PushrebaseParams};
use mononoke_types::{BonsaiChangeset, ChangesetId};
use reachabilityindex::LeastCommonAncestorsHint;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use scuba_ext::ScubaSampleBuilderExt;
use skeleton_manifest::RootSkeletonManifestId;
use tunables::tunables;

use crate::hook_running::run_hooks;
use crate::restrictions::{BookmarkKind, BookmarkMoveAuthorization};
use crate::BookmarkMovementError;

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
        repo: &BlobRepo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        bookmark_attrs: &BookmarkAttrs,
        bookmark: &BookmarkName,
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

        let mut exclude_bookmarks: HashSet<_> = bookmark_attrs
            .select(bookmark)
            .map(|params| params.hooks_skip_ancestors_of.iter())
            .flatten()
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

        let range = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
            ctx.clone(),
            &repo.get_changeset_fetcher(),
            lca_hint.clone(),
            vec![head],
            excludes.into_iter().collect(),
        )
        .compat()
        .try_filter(|bcs_id| {
            let exists = self.new_changesets.contains_key(bcs_id);
            future::ready(!exists)
        });

        let limit = match tunables().get_hooks_additional_changesets_limit() {
            limit if limit > 0 => limit as usize,
            _ => std::usize::MAX,
        };

        let additional_changesets = if tunables().get_run_hooks_on_additional_changesets() {
            let bonsais = range
                .and_then({
                    let mut count = 0;
                    move |bcs_id| {
                        count += 1;
                        if count > limit {
                            future::ready(Err(anyhow!(
                                "hooks additional changesets limit reached at {}",
                                bcs_id
                            )))
                        } else {
                            future::ready(Ok(bcs_id))
                        }
                    }
                })
                .map(|res| async move {
                    match res {
                        Ok(bcs_id) => Ok(bcs_id.load(ctx, repo.blobstore()).await?),
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
                .take(limit)
                .try_fold(0usize, |acc, _| async move { Ok(acc + 1) })
                .await?;

            let mut scuba = ctx.scuba().clone();
            scuba.add("hook_running_additional_changesets", count);
            if count >= limit {
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
        repo: &BlobRepo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        pushrebase_params: &PushrebaseParams,
        bookmark_attrs: &BookmarkAttrs,
        hook_manager: &HookManager,
        bookmark: &BookmarkName,
        pushvars: Option<&HashMap<String, Bytes>>,
        reason: BookmarkUpdateReason,
        kind: BookmarkKind,
        auth: &BookmarkMoveAuthorization<'_>,
        additional_changesets: AdditionalChangesets,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        self.check_case_conflicts(
            ctx,
            repo,
            lca_hint,
            pushrebase_params,
            bookmark_attrs,
            bookmark,
            kind,
            additional_changesets,
        )
        .await?;

        self.check_hooks(
            ctx,
            repo,
            lca_hint,
            bookmark_attrs,
            hook_manager,
            bookmark,
            pushvars,
            reason,
            kind,
            auth,
            additional_changesets,
            cross_repo_push_source,
        )
        .await?;

        self.check_service_write_restrictions(
            ctx,
            repo,
            lca_hint,
            bookmark_attrs,
            bookmark,
            auth,
            additional_changesets,
        )
        .await?;

        Ok(())
    }

    /// If the push is to a public bookmark, and the casefolding check is
    /// enabled, check that no affected changeset has case conflicts.
    async fn check_case_conflicts(
        &mut self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        pushrebase_params: &PushrebaseParams,
        bookmark_attrs: &BookmarkAttrs,
        bookmark: &BookmarkName,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        if kind == BookmarkKind::Public
            && pushrebase_params.flags.casefolding_check
            && tunables().get_check_case_conflicts_on_bookmark_movement()
        {
            self.load_additional_changesets(
                ctx,
                repo,
                lca_hint,
                bookmark_attrs,
                bookmark,
                additional_changesets,
            )
            .await
            .context("Failed to load additional affected changesets")?;

            let sk_mf_ids = RootSkeletonManifestId::batch_derive(
                ctx,
                repo,
                self.iter().map(BonsaiChangeset::get_changeset_id),
            )
            .await?;
            stream::iter(sk_mf_ids.into_iter().map(Ok))
                .try_for_each_concurrent(100, |(bcs_id, sk_mf_id)| async move {
                    let sk_mf = sk_mf_id
                        .into_skeleton_manifest_id()
                        .load(ctx, repo.blobstore())
                        .await
                        .map_err(Error::from)?;
                    if let Some((path1, path2)) =
                        sk_mf.first_case_conflict(ctx, repo.blobstore()).await?
                    {
                        return Err(BookmarkMovementError::CaseConflict {
                            changeset_id: bcs_id,
                            path1,
                            path2,
                        });
                    }
                    Ok(())
                })
                .await?;
        }
        Ok(())
    }

    /// If this is a user-initiated update to a public bookmark, run the
    /// hooks against the affected changesets.
    async fn check_hooks(
        &mut self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        bookmark_attrs: &BookmarkAttrs,
        hook_manager: &HookManager,
        bookmark: &BookmarkName,
        pushvars: Option<&HashMap<String, Bytes>>,
        reason: BookmarkUpdateReason,
        kind: BookmarkKind,
        auth: &BookmarkMoveAuthorization<'_>,
        additional_changesets: AdditionalChangesets,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        if auth == &BookmarkMoveAuthorization::User && kind == BookmarkKind::Public {
            if reason == BookmarkUpdateReason::Push && tunables().get_disable_hooks_on_plain_push()
            {
                // Skip running hooks for this plain push.
                return Ok(());
            }

            if hook_manager.hooks_exist_for_bookmark(bookmark) {
                self.load_additional_changesets(
                    ctx,
                    repo,
                    lca_hint,
                    bookmark_attrs,
                    bookmark,
                    additional_changesets,
                )
                .await
                .context("Failed to load additional affected changesets")?;

                if !self.is_empty() {
                    run_hooks(
                        ctx,
                        hook_manager,
                        bookmark,
                        self.iter(),
                        pushvars,
                        cross_repo_push_source,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// If this is service-initiated update to a bookmark, check the update's
    /// affected changesets satisfy the service write restrictions.
    async fn check_service_write_restrictions(
        &mut self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        bookmark_attrs: &BookmarkAttrs,
        bookmark: &BookmarkName,
        auth: &BookmarkMoveAuthorization<'_>,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        if let BookmarkMoveAuthorization::Service(service_name, scs_params) = auth {
            if scs_params.service_write_all_paths_permitted(service_name) {
                return Ok(());
            }

            self.load_additional_changesets(
                ctx,
                repo,
                lca_hint,
                bookmark_attrs,
                bookmark,
                additional_changesets,
            )
            .await
            .context("Failed to load additional affected changesets")?;

            for cs in self.iter() {
                if let Err(path) = scs_params.service_write_paths_permitted(service_name, cs) {
                    return Err(BookmarkMovementError::PermissionDeniedServicePath {
                        service_name: service_name.clone(),
                        path: path.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}
