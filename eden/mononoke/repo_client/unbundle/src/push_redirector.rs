/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use backsyncer::backsync_latest;
use backsyncer::BacksyncLimit;
use blobstore::Loadable;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use cacheblob::LeaseOps;
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::types::Target;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use futures::future::try_join_all;
use futures::future::FutureExt;
use futures::try_join;
use hooks::CrossRepoPushSource;
use hooks::HookManagerRef;
use hooks::HookRejection;
use live_commit_sync_config::LiveCommitSyncConfig;
use mercurial_derived_data::DeriveHgChangeset;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use pushrebase::PushrebaseChangesetPair;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_update_logger::log_new_commits;
use repo_update_logger::CommitInfo;
use skiplist::SkiplistIndexArc;
use slog::debug;
use synced_commit_mapping::SyncedCommitMapping;
use topo_sort::sort_topological;
use wireproto_handler::TargetRepoDbs;

use crate::hook_running::HookRejectionRemapper;
use crate::resolver::HgHookRejection;
use crate::run_post_resolve_action;
use crate::BundleResolverError;
use crate::InfiniteBookmarkPush;
use crate::PlainBookmarkPush;
use crate::PostResolveAction;
use crate::PostResolveBookmarkOnlyPushRebase;
use crate::PostResolveInfinitePush;
use crate::PostResolvePush;
use crate::PostResolvePushRebase;
use crate::PushrebaseBookmarkSpec;
use crate::UnbundleBookmarkOnlyPushRebaseResponse;
use crate::UnbundleInfinitePushResponse;
use crate::UnbundlePushRebaseResponse;
use crate::UnbundlePushResponse;
use crate::UnbundleResponse;
use crate::UploadedBonsais;

pub trait Repo = crate::processing::Repo + SkiplistIndexArc + HookManagerRef;

/// An auxillary struct, which contains nearly
/// everything needed to create a full `PushRedirector`
/// This is intended to be used to create a new
/// `PushRedirector` at the start of every `unbundle`
/// request.
pub struct PushRedirectorArgs<R> {
    target_repo: Arc<R>,
    source_repo: Arc<R>,
    synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
    target_repo_dbs: Arc<TargetRepoDbs>,
}

// Implement clone manually because a derived implementation would require
// `R: Clone`, which we do not have.
// https://github.com/rust-lang/rust/issues/26925
impl<R> Clone for PushRedirectorArgs<R> {
    fn clone(&self) -> Self {
        Self {
            target_repo: self.target_repo.clone(),
            source_repo: self.source_repo.clone(),
            synced_commit_mapping: self.synced_commit_mapping.clone(),
            target_repo_dbs: self.target_repo_dbs.clone(),
        }
    }
}

impl<R: Repo> PushRedirectorArgs<R> {
    pub fn new(
        target_repo: Arc<R>,
        source_repo: Arc<R>,
        synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
        target_repo_dbs: Arc<TargetRepoDbs>,
    ) -> Self {
        Self {
            target_repo,
            source_repo,
            synced_commit_mapping,
            target_repo_dbs,
        }
    }

    /// Create `PushRedirector` for a given source repo
    pub fn into_push_redirector(
        self,
        ctx: &CoreContext,
        live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
        x_repo_sync_lease: Arc<dyn LeaseOps>,
    ) -> Result<PushRedirector<R>, Error> {
        // TODO: This function needs to be extended
        //       and query configerator for the fresh
        //       value of `commit_sync_config`
        let PushRedirectorArgs {
            target_repo,
            source_repo,
            synced_commit_mapping,
            target_repo_dbs,
            ..
        } = self;

        let small_repo = source_repo.as_blob_repo().clone();
        let large_repo = target_repo.as_blob_repo().clone();
        let syncers = create_commit_syncers(
            ctx,
            small_repo,
            large_repo,
            synced_commit_mapping,
            live_commit_sync_config,
            x_repo_sync_lease,
        )?;

        let small_to_large_commit_syncer = syncers.small_to_large;
        let large_to_small_commit_syncer = syncers.large_to_small;

        debug!(ctx.logger(), "Instantiating a new PushRedirector");
        Ok(PushRedirector {
            repo: target_repo,
            small_repo: source_repo,
            small_to_large_commit_syncer,
            large_to_small_commit_syncer,
            target_repo_dbs,
        })
    }
}

/// Core push redirector struct. Performs conversions of pushes
/// to be processed by the large repo, and conversions of results
/// to be presented as if the pushes were processed by the small repo
pub struct PushRedirector<R> {
    // target (large) repo to sync into
    pub repo: Arc<R>,
    // small repo to sync from
    pub small_repo: Arc<R>,
    // `CommitSyncer` struct to do push redirecion
    pub small_to_large_commit_syncer: CommitSyncer<Arc<dyn SyncedCommitMapping>>,
    // `CommitSyncer` struct for the backsyncer
    pub large_to_small_commit_syncer: CommitSyncer<Arc<dyn SyncedCommitMapping>>,
    // A struct, needed to backsync commits
    pub target_repo_dbs: Arc<TargetRepoDbs>,
}

impl<R: Repo> PushRedirector<R> {
    /// To the external observer, this fn is just like `run_post_resolve_action`
    /// in that it will result in the repo having the action processed.
    /// Under the hood it will:
    /// - convert small repo `PostResolveAction` into a large repo `PostResolveAction`
    /// - run the result of this conversion against the large repo
    /// - trigger a commit backsyncing into the small repo
    /// - convert the `UnbundleResponse` struct to be a small-repo one
    pub async fn run_redirected_post_resolve_action(
        &self,
        ctx: &CoreContext,
        action: PostResolveAction,
    ) -> Result<UnbundleResponse, BundleResolverError> {
        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = self.repo.skiplist_index_arc();

        let maybe_changesets_to_log: Option<HashMap<ChangesetId, CommitInfo>> = match &action {
            PostResolveAction::PushRebase(action) => Some(
                action
                    .uploaded_bonsais
                    .iter()
                    .map(|bcs| (bcs.get_changeset_id(), CommitInfo::new(bcs, None)))
                    .collect(),
            ),
            _ => None,
        };

        let large_repo_action = self
            .convert_post_resolve_action(ctx, action)
            .await
            .map_err(BundleResolverError::from)?;

        let large_repo_response = run_post_resolve_action(
            ctx,
            self.repo.as_ref(),
            &lca_hint,
            self.repo.hook_manager(),
            large_repo_action,
            CrossRepoPushSource::PushRedirected,
        )
        .await?;
        self.convert_unbundle_response(ctx, large_repo_response, maybe_changesets_to_log)
            .await
            .map_err(BundleResolverError::from)
    }

    fn make_hook_rejection_remapper(
        &self,
        ctx: &CoreContext,
        large_to_small: HashMap<ChangesetId, ChangesetId>,
    ) -> Arc<dyn HookRejectionRemapper> {
        Arc::new({
            let large_to_small_commit_syncer = self.large_to_small_commit_syncer.clone();
            cloned!(ctx);
            move |HookRejection {
                      hook_name,
                      cs_id,
                      reason,
                  }| {
                cloned!(ctx, large_to_small_commit_syncer, large_to_small);
                // For the benefit of the user seeing the error, remap the commit hash back
                // to the small repo, so that while the error message may contain large repo
                // paths, the commit hash is the one you have in your small repo
                async move {
                    let small_repo = large_to_small_commit_syncer.get_target_repo();
                    let large_repo = large_to_small_commit_syncer.get_source_repo();
                    let (repo, cs_id) = match large_to_small.get(&cs_id) {
                        Some(&small_cs_id) => (small_repo.clone(), small_cs_id),
                        None => match large_to_small_commit_syncer
                            .get_commit_sync_outcome(&ctx, cs_id)
                            .await?
                        {
                            Some(CommitSyncOutcome::RewrittenAs(small_cs_id, _)) => {
                                (small_repo.clone(), small_cs_id)
                            }
                            _ => {
                                // The changeset doesn't map into the small
                                // repo.  Just use the large repo's changeset
                                // id.
                                (large_repo.clone(), cs_id)
                            }
                        },
                    };

                    let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;

                    Ok(HgHookRejection {
                        hook_name,
                        hg_cs_id,
                        reason,
                    })
                }
                .boxed()
            }
        })
    }

    /// Convert `PostResolveAction` enum in a small-to-large direction
    /// to be suitable for processing in the large repo
    async fn convert_post_resolve_action(
        &self,
        ctx: &CoreContext,
        orig: PostResolveAction,
    ) -> Result<PostResolveAction, Error> {
        use PostResolveAction::*;
        match orig {
            Push(action) => self
                .convert_post_resolve_push_action(ctx, action)
                .await
                .map(Push),
            PushRebase(action) => self
                .convert_post_resolve_pushrebase_action(ctx, action)
                .await
                .map(PushRebase),
            InfinitePush(action) => self
                .convert_post_resolve_infinitepush_action(ctx, action)
                .await
                .map(InfinitePush),
            BookmarkOnlyPushRebase(action) => self
                .convert_post_resolve_bookmark_only_pushrebase_action(ctx, action)
                .await
                .map(BookmarkOnlyPushRebase),
        }
    }

    /// Convert `PostResolvePush` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_push_action(
        &self,
        ctx: &CoreContext,
        orig: PostResolvePush,
    ) -> Result<PostResolvePush, Error> {
        // Note: the `maybe_raw_bundle2_id` field here contains a bundle, which
        // was uploaded in the small repo (and is stored in the small repo's blobstore).
        // However, once the `bookmarks_update_log` transaction is successful, we
        // will mention this bundle id in the table entry. In essense, the table
        // entry for the large repo will point to a blobstore key, which does not
        // exist in that large repo.
        let PostResolvePush {
            changegroup_id,
            bookmark_pushes,
            mutations: _,
            maybe_pushvars,
            non_fast_forward_policy,
            uploaded_bonsais,
            uploaded_hg_changeset_ids: _,
            hook_rejection_remapper: _,
        } = orig;

        let uploaded_bonsais = self
            .sync_uploaded_changesets(ctx, uploaded_bonsais, None)
            .await?;

        let bookmark_pushes = try_join_all(bookmark_pushes.into_iter().map(|bookmark_push| {
            self.convert_plain_bookmark_push_small_to_large(ctx, bookmark_push)
        }))
        .await?;

        let large_to_small = uploaded_bonsais
            .iter()
            .map(|(small_cs_id, large_bcs)| (large_bcs.get_changeset_id(), *small_cs_id))
            .collect::<HashMap<_, _>>();

        let hook_rejection_remapper = self.make_hook_rejection_remapper(ctx, large_to_small);

        Ok(PostResolvePush {
            changegroup_id,
            bookmark_pushes,
            mutations: Default::default(),
            maybe_pushvars,
            non_fast_forward_policy,
            uploaded_bonsais: uploaded_bonsais.values().cloned().collect(),
            uploaded_hg_changeset_ids: Default::default(),
            hook_rejection_remapper,
        })
    }

    /// Convert `PostResolvePushRebase` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_pushrebase_action(
        &self,
        ctx: &CoreContext,
        orig: PostResolvePushRebase,
    ) -> Result<PostResolvePushRebase, Error> {
        // Note: the `maybe_raw_bundle2_id` field here contains a bundle, which
        // was uploaded in the small repo (and is stored in the small repo's blobstore).
        // However, once the `bookmarks_update_log` transaction is successful, we
        // will mention this bundle id in the table entry. In essense, the table
        // entry for the large repo will point to a blobstore key, which does not
        // exist in that large repo.
        let PostResolvePushRebase {
            bookmark_push_part_id,
            bookmark_spec,
            maybe_pushvars,
            commonheads,
            uploaded_bonsais,
            hook_rejection_remapper: _,
        } = orig;

        // We cannot yet call `convert_pushrebase_bookmark_spec`, as that fn requires
        // changesets to be rewritten
        let maybe_renamed_bookmark = self
            .small_to_large_commit_syncer
            .rename_bookmark(bookmark_spec.get_bookmark_name())
            .await?;

        let uploaded_bonsais = self
            .sync_uploaded_changesets(ctx, uploaded_bonsais, maybe_renamed_bookmark.as_ref())
            .await?;

        let bookmark_spec = self
            .convert_pushrebase_bookmark_spec(ctx, bookmark_spec)
            .await?;

        let large_to_small = uploaded_bonsais
            .iter()
            .map(|(small_cs_id, large_bcs)| (large_bcs.get_changeset_id(), *small_cs_id))
            .collect::<HashMap<_, _>>();

        let hook_rejection_remapper = self.make_hook_rejection_remapper(ctx, large_to_small);

        let action = PostResolvePushRebase {
            bookmark_push_part_id,
            bookmark_spec,
            maybe_pushvars,
            commonheads,
            uploaded_bonsais: uploaded_bonsais.values().cloned().collect(),
            hook_rejection_remapper,
        };

        Ok(action)
    }

    /// Convert `PostResolveInfinitePush` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_infinitepush_action(
        &self,
        ctx: &CoreContext,
        orig: PostResolveInfinitePush,
    ) -> Result<PostResolveInfinitePush, Error> {
        let PostResolveInfinitePush {
            changegroup_id,
            maybe_bookmark_push,
            mutations: _,
            uploaded_bonsais,
            uploaded_hg_changeset_ids: _,
        } = orig;
        let uploaded_bonsais = self
            .sync_uploaded_changesets(ctx, uploaded_bonsais, None)
            .await?;
        let maybe_bookmark_push = match maybe_bookmark_push {
            Some(bookmark_push) => Some(
                self.convert_infinite_bookmark_push_small_to_large(ctx, bookmark_push)
                    .await
                    .context("while converting infinite bookmark push small-to-large")?,
            ),
            None => None,
        };

        Ok(PostResolveInfinitePush {
            changegroup_id,
            maybe_bookmark_push,
            mutations: Default::default(),
            uploaded_bonsais: uploaded_bonsais.values().cloned().collect(),
            uploaded_hg_changeset_ids: Default::default(),
        })
    }

    /// Convert a `PostResolveBookmarkOnlyPushRebase` in a small-to-large
    /// direction, to be suitable for a processing in a large repo
    async fn convert_post_resolve_bookmark_only_pushrebase_action(
        &self,
        ctx: &CoreContext,
        orig: PostResolveBookmarkOnlyPushRebase,
    ) -> Result<PostResolveBookmarkOnlyPushRebase, Error> {
        let PostResolveBookmarkOnlyPushRebase {
            bookmark_push,
            maybe_pushvars,
            non_fast_forward_policy,
            hook_rejection_remapper: _,
        } = orig;

        let bookmark_push = self
            .convert_plain_bookmark_push_small_to_large(ctx, bookmark_push)
            .await
            .context("while converting converting plain bookmark push small-to-large")?;

        let hook_rejection_remapper = self.make_hook_rejection_remapper(ctx, HashMap::new());

        Ok(PostResolveBookmarkOnlyPushRebase {
            bookmark_push,
            maybe_pushvars,
            non_fast_forward_policy,
            hook_rejection_remapper,
        })
    }

    /// Convert `UnbundleResponse` enum in a large-to-small direction
    /// to be suitable for response generation in the small repo
    async fn convert_unbundle_response(
        &self,
        ctx: &CoreContext,
        orig: UnbundleResponse,
        maybe_changesets_to_log: Option<HashMap<ChangesetId, CommitInfo>>,
    ) -> Result<UnbundleResponse, Error> {
        use UnbundleResponse::*;
        match orig {
            PushRebase(resp) => Ok(PushRebase(
                self.convert_unbundle_pushrebase_response(ctx, resp, maybe_changesets_to_log)
                    .await
                    .context("while converting unbundle pushrebase response")?,
            )),
            BookmarkOnlyPushRebase(resp) => Ok(BookmarkOnlyPushRebase(
                self.convert_unbundle_bookmark_only_pushrebase_response(ctx, resp)
                    .await
                    .context("while converting unbundle bookmark-only pushrebase response")?,
            )),
            Push(resp) => Ok(Push(
                self.convert_unbundle_push_response(ctx, resp)
                    .await
                    .context("while converting unbundle push response")?,
            )),
            InfinitePush(resp) => Ok(InfinitePush(
                self.convert_unbundle_infinite_push_response(ctx, resp)
                    .await
                    .context("while converting unbundle infinitepush response")?,
            )),
        }
    }

    pub async fn backsync_latest(&self, ctx: &CoreContext) -> Result<(), Error> {
        backsync_latest(
            ctx.clone(),
            self.large_to_small_commit_syncer.clone(),
            self.target_repo_dbs.clone(),
            BacksyncLimit::NoLimit,
            Arc::new(AtomicBool::new(false)),
            CommitSyncContext::PushRedirector,
            // Disable leases because this is the push path and we want
            // it as fast as possible
            true,
        )
        .await?;
        Ok(())
    }

    pub async fn small_to_large_bookmark(
        &self,
        bookmark: &BookmarkName,
    ) -> Result<BookmarkName, Error> {
        self.small_to_large_commit_syncer
            .rename_bookmark(bookmark)
            .await?
            .with_context(|| {
                format!(
                    "Bookmark {} unexpectedly dropped in {:?}",
                    bookmark, self.small_to_large_commit_syncer
                )
            })
    }

    /// Convert `UnbundlePushRebaseResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_pushrebase_response(
        &self,
        ctx: &CoreContext,
        orig: UnbundlePushRebaseResponse,
        maybe_changesets_to_log: Option<HashMap<ChangesetId, CommitInfo>>,
    ) -> Result<UnbundlePushRebaseResponse, Error> {
        let UnbundlePushRebaseResponse {
            commonheads,
            pushrebased_rev,
            pushrebased_changesets,
            onto,
            bookmark_push_part_id,
        } = orig;

        // Let's make sure all the public pushes to the large repo
        // are backsynced to the small repo, by tailing the `bookmarks_update_log`
        // of the large repo
        self.backsync_latest(ctx).await?;

        let (pushrebased_rev, pushrebased_changesets) = try_join!(
            async {
                self.get_large_to_small_commit_equivalent(ctx, pushrebased_rev)
                    .await
                    .context("while remapping pushrebased rev")
            },
            async {
                self.convert_pushrebased_changesets(ctx, pushrebased_changesets)
                    .await
                    .context("while converting pushrebased changesets")
            },
        )?;

        let mut changesets_to_log = maybe_changesets_to_log.unwrap_or_default();
        for pair in pushrebased_changesets.iter() {
            let info = changesets_to_log
                .get_mut(&pair.id_old)
                .with_context(|| format!("Missing commit info for {}", pair.id_old))?;
            info.update_changeset_id(pair.id_old, pair.id_new)?;
        }

        let onto = self
            .large_to_small_commit_syncer
            .rename_bookmark(&onto)
            .await?
            .ok_or_else(|| {
                format_err!(
                    "bookmark_renamer unexpectedly dropped {} in {:?}",
                    onto,
                    self.large_to_small_commit_syncer
                )
            })?;

        // Also log commits on small repo
        log_new_commits(
            ctx,
            self.small_repo.as_ref(),
            Some((&onto, BookmarkKind::Publishing)),
            changesets_to_log.into_values().collect(),
        )
        .await;

        Ok(UnbundlePushRebaseResponse {
            commonheads,
            pushrebased_rev,
            pushrebased_changesets,
            onto,
            bookmark_push_part_id,
        })
    }

    /// Convert `UnbundleBookmarkOnlyPushRebaseResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_bookmark_only_pushrebase_response(
        &self,
        ctx: &CoreContext,
        orig: UnbundleBookmarkOnlyPushRebaseResponse,
    ) -> Result<UnbundleBookmarkOnlyPushRebaseResponse, Error> {
        // `UnbundleBookmarkOnlyPushRebaseResponse` consists of only one field:
        // `bookmark_push_part_id`, which does not need to be converted
        // We do, however, need to wait until the backsyncer catches up with
        // with the `bookmarks_update_log` tailing
        self.backsync_latest(ctx).await?;

        Ok(orig)
    }

    /// Convert `UnbundlePushResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_push_response(
        &self,
        ctx: &CoreContext,
        orig: UnbundlePushResponse,
    ) -> Result<UnbundlePushResponse, Error> {
        // `UnbundlePushResponse` consists of only two fields:
        // `changegroup_id` and `bookmark_ids`, which do not need to be converted
        // We do, however, need to wait until the backsyncer catches up with
        // with the `bookmarks_update_log` tailing
        self.backsync_latest(ctx).await?;

        Ok(orig)
    }

    /// Convert `UnbundleInfinitePushResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_infinite_push_response(
        &self,
        _ctx: &CoreContext,
        _orig: UnbundleInfinitePushResponse,
    ) -> Result<UnbundleInfinitePushResponse, Error> {
        // TODO: this can only be implemented once we have a way
        //       catch up on non-public commits, created in the
        //       large repo. One proposal is to include those in
        //       `UnbundleInfinitePushResponse` and make this
        //       method call some `CommitSyncer` method to sync
        //       those commits.
        Err(format_err!(
            "convert_unbundle_infinite_push_response is not implemented"
        ))
    }

    /// Given, the `source_cs_id` in the small repo, get it's equivalent
    /// in a large repo. See `remap_changeset_expect_rewritten_or_preserved`
    /// for details
    pub async fn get_small_to_large_commit_equivalent(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<ChangesetId, Error> {
        self.remap_changeset_expect_rewritten_or_preserved(
            ctx,
            &self.small_to_large_commit_syncer,
            source_cs_id,
        )
        .await
    }

    /// Given, the `source_cs_id` in the large repo, get it's equivalent
    /// in a small repo. See `remap_changeset_expect_rewritten_or_preserved`
    /// for details
    pub async fn get_large_to_small_commit_equivalent(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<ChangesetId, Error> {
        self.remap_changeset_expect_rewritten_or_preserved(
            ctx,
            &self.large_to_small_commit_syncer,
            source_cs_id,
        )
        .await
    }

    /// Query the changeset mapping from the provided `syncer`
    /// Error out if the `CommitSyncOutcome` is not `RewrittenAs`
    /// The logic of this method is to express an expectation that `cs_id`
    /// from the source repo MUST be properly present in the target repo,
    /// either with paths moved, or preserved. What is unacceptable is that
    /// the changeset is not yet synced, or rewritten into nothingness, or
    /// preserved from a different repo.
    async fn remap_changeset_expect_rewritten_or_preserved(
        &self,
        ctx: &CoreContext,
        syncer: &CommitSyncer<Arc<dyn SyncedCommitMapping>>,
        cs_id: ChangesetId,
    ) -> Result<ChangesetId, Error> {
        let maybe_commit_sync_outcome = syncer.get_commit_sync_outcome(ctx, cs_id).await?;
        maybe_commit_sync_outcome
            .ok_or_else(|| {
                format_err!(
                    "Unexpected absence of CommitSyncOutcome for {} in {:?}",
                    cs_id,
                    syncer
                )
            })
            .and_then(|commit_sync_outcome| match commit_sync_outcome {
                CommitSyncOutcome::RewrittenAs(rewritten, _) => Ok(rewritten),
                cso => Err(format_err!(
                    "Unexpected CommitSyncOutcome for {} in {:?}: {:?}",
                    cs_id,
                    syncer,
                    cso
                )),
            })
    }

    /// Convert `InfiniteBookmarkPush<ChangesetId>` in the small-to-large direction
    /// Note: this does not cause any changesets to be synced, just converts the struct
    ///       all the syncing is expected to be done prior to calling this fn.
    async fn convert_infinite_bookmark_push_small_to_large(
        &self,
        ctx: &CoreContext,
        orig: InfiniteBookmarkPush<ChangesetId>,
    ) -> Result<InfiniteBookmarkPush<ChangesetId>, Error> {
        let maybe_old = orig.old.clone();
        let new = orig.new.clone();

        let (old, new) = try_join!(
            async {
                match maybe_old {
                    None => Ok(None),
                    Some(old) => self
                        .get_small_to_large_commit_equivalent(ctx, old)
                        .await
                        .map(Some),
                }
            },
            self.get_small_to_large_commit_equivalent(ctx, new),
        )?;

        Ok(InfiniteBookmarkPush { old, new, ..orig })
    }

    /// Convert `PlainBookmarkPush<ChangesetId>` in the small-to-large direction
    /// Note: this does not cause any changesets to be synced, just converts the struct
    ///       all the syncing is expected to be done prior to calling this fn.
    async fn convert_plain_bookmark_push_small_to_large(
        &self,
        ctx: &CoreContext,
        orig: PlainBookmarkPush<ChangesetId>,
    ) -> Result<PlainBookmarkPush<ChangesetId>, Error> {
        let PlainBookmarkPush {
            part_id,
            name,
            old: maybe_old,
            new: maybe_new,
        } = orig;

        if self
            .small_to_large_commit_syncer
            .get_common_pushrebase_bookmarks()
            .await?
            .contains(&name)
        {
            return Err(format_err!(
                "cannot force pushrebase to shared bookmark {}",
                name
            ));
        }

        let (old, new) = try_join!(
            async {
                match maybe_old {
                    None => Ok(None),
                    Some(old) => self
                        .get_small_to_large_commit_equivalent(ctx, old)
                        .await
                        .map(Some),
                }
            },
            async {
                match maybe_new {
                    None => Ok(None),
                    Some(new) => self
                        .get_small_to_large_commit_equivalent(ctx, new)
                        .await
                        .map(Some),
                }
            },
        )?;

        let name = self.small_to_large_bookmark(&name).await?;

        Ok(PlainBookmarkPush {
            part_id,
            name,
            old,
            new,
        })
    }

    /// Convert the `PushrebaseBookmarkSpec` struct in the small-to-large direction
    async fn convert_pushrebase_bookmark_spec(
        &self,
        ctx: &CoreContext,
        pushrebase_bookmark_spec: PushrebaseBookmarkSpec<ChangesetId>,
    ) -> Result<PushrebaseBookmarkSpec<ChangesetId>, Error> {
        match pushrebase_bookmark_spec {
            PushrebaseBookmarkSpec::NormalPushrebase(bookmark) => {
                let bookmark = self.small_to_large_bookmark(&bookmark).await?;

                Ok(PushrebaseBookmarkSpec::NormalPushrebase(bookmark))
            }
            PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => {
                let converted = self
                    .convert_plain_bookmark_push_small_to_large(ctx, plain_push)
                    .await?;
                Ok(PushrebaseBookmarkSpec::ForcePushrebase(converted))
            }
        }
    }

    /// Convert `PushrebaseChangesetPair` struct in the large-to-small direction
    async fn convert_pushrebase_changeset_pair(
        &self,
        ctx: &CoreContext,
        pushrebase_changeset_pair: PushrebaseChangesetPair,
    ) -> Result<PushrebaseChangesetPair, Error> {
        let PushrebaseChangesetPair { id_old, id_new } = pushrebase_changeset_pair;
        let (id_old, id_new) = try_join!(
            self.get_large_to_small_commit_equivalent(ctx, id_old),
            self.get_large_to_small_commit_equivalent(ctx, id_new),
        )?;
        Ok(PushrebaseChangesetPair { id_old, id_new })
    }

    /// Convert all the produced `PushrebaseChangesetPair` structs in the
    /// large-to-small direction
    pub async fn convert_pushrebased_changesets(
        &self,
        ctx: &CoreContext,
        pushrebased_changesets: Vec<PushrebaseChangesetPair>,
    ) -> Result<Vec<PushrebaseChangesetPair>, Error> {
        try_join_all(pushrebased_changesets.into_iter().map({
            |pushrebase_changeset_pair| {
                self.convert_pushrebase_changeset_pair(ctx, pushrebase_changeset_pair)
            }
        }))
        .await
    }

    /// Take changesets uploaded during the `unbundle` resolution
    /// and sync all the changesets into a large repo, while remembering which small cs id
    /// corresponds to which large cs id
    pub async fn sync_uploaded_changesets(
        &self,
        ctx: &CoreContext,
        uploaded_map: UploadedBonsais,
        maybe_bookmark: Option<&BookmarkName>,
    ) -> Result<HashMap<ChangesetId, BonsaiChangeset>, Error> {
        let target_repo = self.small_to_large_commit_syncer.get_target_repo();
        let uploaded_ids: HashSet<ChangesetId> = uploaded_map
            .iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect();

        let to_sync: HashMap<ChangesetId, Vec<ChangesetId>> = uploaded_map
            .iter()
            .map(|bcs| {
                // For the toposort purposes, let's only collect parents, uploaded
                // as part of this push
                let uploaded_parents: Vec<ChangesetId> = bcs
                    .parents()
                    .filter(|bcs_id| uploaded_ids.contains(bcs_id))
                    .collect();
                (bcs.get_changeset_id(), uploaded_parents)
            })
            .collect();

        let to_sync: Vec<ChangesetId> = sort_topological(&to_sync)
            .ok_or_else(|| format_err!("Cycle in the uploaded changeset DAG!"))?
            .into_iter()
            .collect();

        let mut synced_ids = Vec::new();

        // Only when we know the target bookmark, we tell the mapping logic
        // to look for its ancestor  if small repo commit rewrites into multiple
        // large repo commits.
        let candidate_selection_hint = match maybe_bookmark {
            Some(bookmark) => CandidateSelectionHint::OnlyOrAncestorOfBookmark(
                Target(bookmark.clone()),
                Target(self.small_to_large_commit_syncer.get_target_repo().clone()),
                Target(self.repo.skiplist_index_arc()),
            ),
            None => CandidateSelectionHint::Only,
        };

        for bcs_id in to_sync.iter() {
            let synced_bcs_id = self
                .small_to_large_commit_syncer
                .unsafe_sync_commit(
                    ctx,
                    *bcs_id,
                    candidate_selection_hint.clone(),
                    CommitSyncContext::PushRedirector,
                )
                .await?
                .ok_or_else(|| {
                    format_err!(
                        "{} was rewritten into nothingness during uploaded changesets sync",
                        bcs_id
                    )
                })?;
            synced_ids.push((bcs_id, synced_bcs_id));
        }

        try_join_all(
            synced_ids
                .into_iter()
                .map(move |(small_bcs_id, target_repo_bcs_id)| {
                    cloned!(ctx, target_repo);
                    async move {
                        let target_bcs = target_repo_bcs_id
                            .load(&ctx, target_repo.blobstore())
                            .await?;

                        Ok((*small_bcs_id, target_bcs))
                    }
                }),
        )
        .await
        .map(|v| v.into_iter().collect())
    }
}
