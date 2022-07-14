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

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobrepo::scribe::log_commits_to_scribe_raw;
use blobrepo::scribe::ScribeCommitInfo;
use blobstore::Loadable;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use cross_repo_sync::CHANGE_XREPO_MAPPING_EXTRA;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::FbStreamExt;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use hooks::PushAuthoredBy;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::PushrebaseParams;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_authorization::AuthorizationContext;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use scribe_commit_queue::ChangedFilesInfo;
use skeleton_manifest::RootSkeletonManifestId;
use tunables::tunables;

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
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
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

        let range = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
            ctx.clone(),
            &repo.changeset_fetcher_arc(),
            lca_hint.clone(),
            vec![head],
            excludes.into_iter().collect(),
        )
        .compat()
        .yield_periodically()
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
        authz: &AuthorizationContext,
        repo: &impl Repo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        pushrebase_params: &PushrebaseParams,
        hook_manager: &HookManager,
        bookmark: &BookmarkName,
        pushvars: Option<&HashMap<String, Bytes>>,
        reason: BookmarkUpdateReason,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        self.check_extras(
            ctx,
            repo,
            lca_hint,
            bookmark,
            kind,
            additional_changesets,
            pushrebase_params,
        )
        .await?;

        self.check_case_conflicts(
            ctx,
            repo,
            lca_hint,
            pushrebase_params,
            bookmark,
            kind,
            additional_changesets,
        )
        .await?;

        self.check_hooks(
            ctx,
            authz,
            repo,
            lca_hint,
            hook_manager,
            bookmark,
            pushvars,
            reason,
            kind,
            additional_changesets,
            cross_repo_push_source,
        )
        .await?;

        self.check_path_permissions(ctx, authz, repo, lca_hint, bookmark, additional_changesets)
            .await?;

        Ok(())
    }

    async fn check_extras(
        &mut self,
        ctx: &CoreContext,
        repo: &impl Repo,
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        bookmark: &BookmarkName,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
        pushrebase_params: &PushrebaseParams,
    ) -> Result<(), BookmarkMovementError> {
        if (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && !pushrebase_params.allow_change_xrepo_mapping_extra
        {
            self.load_additional_changesets(ctx, repo, lca_hint, bookmark, additional_changesets)
                .await
                .context("Failed to load additional affected changesets")?;

            for bcs in self.iter() {
                if bcs
                    .extra()
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
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        pushrebase_params: &PushrebaseParams,
        bookmark: &BookmarkName,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        if (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && pushrebase_params.flags.casefolding_check
        {
            self.load_additional_changesets(ctx, repo, lca_hint, bookmark, additional_changesets)
                .await
                .context("Failed to load additional affected changesets")?;

            stream::iter(self.iter().map(Ok))
                .try_for_each_concurrent(100, |bcs| async move {
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

                        if let Some((path1, path2)) = sk_mf
                            .first_new_case_conflict(ctx, repo.repo_blobstore(), parents)
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
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        hook_manager: &HookManager,
        bookmark: &BookmarkName,
        pushvars: Option<&HashMap<String, Bytes>>,
        reason: BookmarkUpdateReason,
        kind: BookmarkKind,
        additional_changesets: AdditionalChangesets,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<(), BookmarkMovementError> {
        if (kind == BookmarkKind::Publishing || kind == BookmarkKind::PullDefaultPublishing)
            && should_run_hooks(authz, reason)
        {
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
                    bookmark,
                    additional_changesets,
                )
                .await
                .context("Failed to load additional affected changesets")?;

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
        lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
        bookmark: &BookmarkName,
        additional_changesets: AdditionalChangesets,
    ) -> Result<(), BookmarkMovementError> {
        // For optimization, first check if the user is permitted to modify
        // all paths.  In that case we don't need to find out which paths were
        // affected.
        if authz.check_any_path_write(ctx, repo).await?.is_denied() {
            // User is not permitted to write to all paths, check if the paths
            // touched by the changesets are permitted.
            self.load_additional_changesets(ctx, repo, lca_hint, bookmark, additional_changesets)
                .await
                .context("Failed to load additional affected changesets")?;

            for cs in self.iter() {
                authz.require_changeset_paths_write(ctx, repo, cs).await?;
            }
        }
        Ok(())
    }
}

pub async fn find_draft_ancestors(
    ctx: &CoreContext,
    repo: &impl Repo,
    to_cs_id: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, Error> {
    ctx.scuba()
        .clone()
        .log_with_msg("Started finding draft ancestors", None);

    let phases = repo.phases();
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut drafts = vec![];
    queue.push_back(to_cs_id);
    visited.insert(to_cs_id);

    while let Some(cs_id) = queue.pop_front() {
        let public = phases
            .get_public(ctx, vec![cs_id], false /*ephemeral_derive*/)
            .await?;

        if public.contains(&cs_id) {
            continue;
        }
        drafts.push(cs_id);

        let parents = repo
            .changeset_fetcher()
            .get_parents(ctx.clone(), cs_id)
            .await?;
        for p in parents {
            if visited.insert(p) {
                queue.push_back(p);
            }
        }
    }

    let drafts = stream::iter(drafts)
        .map(Ok)
        .map_ok(|cs_id| async move { cs_id.load(ctx, repo.repo_blobstore()).await })
        .try_buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;

    ctx.scuba()
        .clone()
        .log_with_msg("Found draft ancestors", Some(format!("{}", drafts.len())));
    Ok(drafts)
}

pub(crate) async fn log_bonsai_commits_to_scribe(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: Option<&BookmarkName>,
    commits_to_log: Vec<BonsaiChangeset>,
    kind: BookmarkKind,
    infinitepush_params: &InfinitepushParams,
    pushrebase_params: &PushrebaseParams,
) {
    let commit_scribe_category = match kind {
        BookmarkKind::Scratch => &infinitepush_params.commit_scribe_category,
        BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
            &pushrebase_params.commit_scribe_category
        }
    };

    log_commits_to_scribe_raw(
        ctx,
        repo,
        bookmark,
        commits_to_log
            .iter()
            .map(|bcs| ScribeCommitInfo {
                changeset_id: bcs.get_changeset_id(),
                bubble_id: None,
                changed_files: ChangedFilesInfo::new(bcs),
            })
            .collect(),
        commit_scribe_category.as_deref(),
    )
    .await;
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::AsBlobRepo;
    use fbinit::FacebookInit;
    use maplit::hashset;
    use mononoke_api_types::InnerRepo;
    use std::collections::HashSet;
    use tests_utils::bookmark;
    use tests_utils::drawdag::create_from_dag;

    #[fbinit::test]
    async fn test_find_draft_ancestors_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: InnerRepo = test_repo_factory::build_empty(fb)?;
        let mapping = create_from_dag(
            &ctx,
            repo.as_blob_repo(),
            r##"
            A-B-C-D
            "##,
        )
        .await?;

        let cs_id = mapping.get("A").unwrap();
        let to_cs_id = mapping.get("D").unwrap();
        bookmark(&ctx, repo.as_blob_repo(), "book")
            .set_to(*cs_id)
            .await?;
        let drafts = find_draft_ancestors(&ctx, &repo, *to_cs_id).await?;

        let drafts = drafts
            .into_iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect::<HashSet<_>>();

        assert_eq!(
            drafts,
            hashset! {
                *mapping.get("B").unwrap(),
                *mapping.get("C").unwrap(),
                *mapping.get("D").unwrap(),
            }
        );

        bookmark(&ctx, repo.as_blob_repo(), "book")
            .set_to(*mapping.get("B").unwrap())
            .await?;
        let drafts = find_draft_ancestors(&ctx, &repo, *to_cs_id).await?;

        let drafts = drafts
            .into_iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect::<HashSet<_>>();

        assert_eq!(
            drafts,
            hashset! {
                *mapping.get("C").unwrap(),
                *mapping.get("D").unwrap(),
            }
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_find_draft_ancestors_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: InnerRepo = test_repo_factory::build_empty(fb)?;
        let mapping = create_from_dag(
            &ctx,
            repo.as_blob_repo(),
            r##"
              B
             /  \
            A    D
             \  /
               C
            "##,
        )
        .await?;

        let cs_id = mapping.get("B").unwrap();
        let to_cs_id = mapping.get("D").unwrap();
        bookmark(&ctx, repo.as_blob_repo(), "book")
            .set_to(*cs_id)
            .await?;
        let drafts = find_draft_ancestors(&ctx, &repo, *to_cs_id).await?;

        let drafts = drafts
            .into_iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect::<HashSet<_>>();

        assert_eq!(
            drafts,
            hashset! {
                *mapping.get("C").unwrap(),
                *mapping.get("D").unwrap(),
            }
        );

        Ok(())
    }
}
