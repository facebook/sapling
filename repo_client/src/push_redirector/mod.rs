/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use crate::mononoke_repo::MononokeRepo;
use crate::unbundle::run_post_resolve_action;
use crate::unbundle::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};

use backsyncer::backsync_all_latest;
use backsyncer::TargetRepoDbs;
use bundle2_resolver::InfiniteBookmarkPush;
use bundle2_resolver::PlainBookmarkPush;
use bundle2_resolver::PushrebaseBookmarkSpec;
use bundle2_resolver::{
    BundleResolverError, PostResolveAction, PostResolveBookmarkOnlyPushRebase,
    PostResolveInfinitePush, PostResolvePush, PostResolvePushRebase, UploadedBonsais,
};
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::{CommitSyncOutcome, CommitSyncer};
use failure_ext::{format_err, Error};
use futures::Future;
use futures_ext::{try_boxfuture, FutureExt as OldFutureExt};
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::try_join_all;
use futures_util::{future::FutureExt, try_future::TryFutureExt, try_join};
use metaconfig_types::CommitSyncConfig;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use pushrebase::{OntoBookmarkParams, PushrebaseChangesetPair};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use synced_commit_mapping::SyncedCommitMapping;
use topo_sort::sort_topological;

pub const CONFIGERATOR_PUSHREDIRECT_ENABLE: &str = "scm/mononoke/pushredirect/enable";

#[derive(Clone)]
/// Core push redirector struct. Performs conversions of pushes
/// to be processed by the large repo, and conversions of results
/// to be presented as if the pushes were processed by the small repo
pub struct RepoSyncTarget {
    // target (large) repo to sync into
    pub repo: MononokeRepo,
    // `CommitSyncer` struct to do push redirecion
    pub small_to_large_commit_syncer: CommitSyncer<Arc<dyn SyncedCommitMapping>>,
    // `CommitSyncer` struct for the backsyncer
    pub large_to_small_commit_syncer: CommitSyncer<Arc<dyn SyncedCommitMapping>>,
    // A struct, needed to backsync commits
    pub target_repo_dbs: TargetRepoDbs,
    // Config for commit sync functionality
    pub commit_sync_config: CommitSyncConfig,
}

impl RepoSyncTarget {
    /// To the external observer, this fn is just like `run_post_resolve_action`
    /// in that it will result in the repo having the action processed.
    /// Under the hood it will:
    /// - convert small repo `PostResolveAction` into a large repo `PostResolveAction`
    /// - run the result of this conversion against the large repo
    /// - trigger a commit backsyncing into the small repo
    /// - convert the `UnbundleResponse` struct to be a small-repo one
    pub fn run_redirected_post_resolve_action_compat(
        self,
        ctx: CoreContext,
        action: PostResolveAction,
    ) -> impl Future<Item = UnbundleResponse, Error = BundleResolverError> {
        async move { self.run_redirected_post_resolve_action(ctx, action).await }
            .boxed()
            .compat()
    }

    /// To the external observer, this fn is just like `run_post_resolve_action`
    /// in that it will result in the repo having the action processed.
    /// Under the hood it will:
    /// - convert small repo `PostResolveAction` into a large repo `PostResolveAction`
    /// - run the result of this conversion against the large repo
    /// - trigger a commit backsyncing into the small repo
    /// - convert the `UnbundleResponse` struct to be a small-repo one
    pub async fn run_redirected_post_resolve_action(
        &self,
        ctx: CoreContext,
        action: PostResolveAction,
    ) -> Result<UnbundleResponse, BundleResolverError> {
        let large_repo = self.repo.blobrepo().clone();
        let bookmark_attrs = self.repo.bookmark_attrs();
        let lca_hint = self.repo.lca_hint();
        let phases = self.repo.phases_hint();
        let infinitepush_params = self.repo.infinitepush().clone();
        let puhsrebase_params = self.repo.pushrebase_params().clone();

        let large_repo_action = self
            .convert_post_resolve_action(ctx.clone(), action)
            .await
            .map_err(BundleResolverError::from)?;
        let large_repo_response = run_post_resolve_action(
            ctx.clone(),
            large_repo,
            bookmark_attrs,
            lca_hint,
            phases,
            infinitepush_params,
            puhsrebase_params,
            large_repo_action,
        )
        .compat()
        .map_err(BundleResolverError::from)
        .await?;
        self.convert_unbundle_response(ctx.clone(), large_repo_response)
            .await
            .map_err(BundleResolverError::from)
    }

    /// Convert `PostResolveAction` enum in a small-to-large direction
    /// to be suitable for processing in the large repo
    async fn convert_post_resolve_action(
        &self,
        ctx: CoreContext,
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
        ctx: CoreContext,
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
            maybe_raw_bundle2_id,
            non_fast_forward_policy,
            uploaded_bonsais,
        } = orig;

        let uploaded_bonsais = self
            .sync_uploaded_changesets(ctx.clone(), uploaded_bonsais)
            .await?;

        let bookmark_pushes = try_join_all(bookmark_pushes.into_iter().map(|bookmark_push| {
            self.convert_plain_bookmark_push_small_to_large(ctx.clone(), bookmark_push)
        }))
        .await?;

        Ok(PostResolvePush {
            changegroup_id,
            bookmark_pushes,
            maybe_raw_bundle2_id,
            non_fast_forward_policy,
            uploaded_bonsais: uploaded_bonsais.values().cloned().map(|bcs| bcs).collect(),
        })
    }

    /// Convert `PostResolvePushRebase` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_pushrebase_action(
        &self,
        ctx: CoreContext,
        orig: PostResolvePushRebase,
    ) -> Result<PostResolvePushRebase, Error> {
        // Note: the `maybe_raw_bundle2_id` field here contains a bundle, which
        // was uploaded in the small repo (and is stored in the small repo's blobstore).
        // However, once the `bookmarks_update_log` transaction is successful, we
        // will mention this bundle id in the table entry. In essense, the table
        // entry for the large repo will point to a blobstore key, which does not
        // exist in that large repo.
        let PostResolvePushRebase {
            any_merges,
            bookmark_push_part_id,
            bookmark_spec,
            maybe_hg_replay_data,
            maybe_pushvars,
            commonheads,
            uploaded_bonsais,
            uploaded_hg_changeset_ids,
        } = orig;

        let uploaded_bonsais = self
            .sync_uploaded_changesets(ctx.clone(), uploaded_bonsais)
            .await?;
        let bookmark_spec = self
            .convert_pushrebase_bookmark_spec(ctx.clone(), bookmark_spec)
            .await?;

        let source_repo = self.small_to_large_commit_syncer.get_source_repo().clone();
        // Pushrebase happens in the large repo, but we'd like to have hg replay data relative
        // to the small repo. In order to do that we need to need to make sure we are using
        // small hg changeset ids for the timestamps instead of large hg changeset ids.
        // In order to do that let's convert large cs id to small cs id and create hg changeset
        // for it.

        let large_to_small = uploaded_bonsais
            .iter()
            .map(|(small_cs_id, large_bcs)| (large_bcs.get_changeset_id(), *small_cs_id))
            .collect::<HashMap<_, _>>();

        let maybe_hg_replay_data = maybe_hg_replay_data.map(|mut hg_replay_data| {
            hg_replay_data.override_convertor(Arc::new({
                move |large_cs_id| {
                    let small_cs_id = try_boxfuture!(large_to_small
                        .get(&large_cs_id)
                        .ok_or(format_err!("{} doesn't remap in small repo", large_cs_id)));
                    source_repo
                        .get_hg_from_bonsai_changeset(ctx.clone(), *small_cs_id)
                        .boxify()
                }
            }));
            hg_replay_data
        });

        Ok(PostResolvePushRebase {
            any_merges,
            bookmark_push_part_id,
            bookmark_spec,
            maybe_hg_replay_data,
            maybe_pushvars,
            commonheads,
            uploaded_bonsais: uploaded_bonsais.values().cloned().map(|bcs| bcs).collect(),
            uploaded_hg_changeset_ids,
        })
    }

    /// Convert `PostResolveInfinitePush` struct in the small-to-large direction
    /// (syncing commits in the process), so that it can be processed in
    /// the large repo
    async fn convert_post_resolve_infinitepush_action(
        &self,
        ctx: CoreContext,
        orig: PostResolveInfinitePush,
    ) -> Result<PostResolveInfinitePush, Error> {
        let PostResolveInfinitePush {
            changegroup_id,
            bookmark_push,
            maybe_raw_bundle2_id,
            uploaded_bonsais,
        } = orig;
        let uploaded_bonsais = self
            .sync_uploaded_changesets(ctx.clone(), uploaded_bonsais)
            .await?;
        let bookmark_push = self
            .convert_infinite_bookmark_push_small_to_large(ctx.clone(), bookmark_push)
            .await?;
        Ok(PostResolveInfinitePush {
            changegroup_id,
            bookmark_push,
            maybe_raw_bundle2_id,
            uploaded_bonsais: uploaded_bonsais.values().cloned().map(|bcs| bcs).collect(),
        })
    }

    /// Convert a `PostResolveBookmarkOnlyPushRebase` in a small-to-large
    /// direction, to be suitable for a processing in a large repo
    async fn convert_post_resolve_bookmark_only_pushrebase_action(
        &self,
        ctx: CoreContext,
        orig: PostResolveBookmarkOnlyPushRebase,
    ) -> Result<PostResolveBookmarkOnlyPushRebase, Error> {
        let PostResolveBookmarkOnlyPushRebase {
            bookmark_push,
            maybe_raw_bundle2_id,
            non_fast_forward_policy,
        } = orig;

        let bookmark_push = self
            .convert_plain_bookmark_push_small_to_large(ctx.clone(), bookmark_push)
            .await?;

        Ok(PostResolveBookmarkOnlyPushRebase {
            bookmark_push,
            maybe_raw_bundle2_id,
            non_fast_forward_policy,
        })
    }

    /// Convert `UnbundleResponse` enum in a large-to-small direction
    /// to be suitable for response generation in the small repo
    async fn convert_unbundle_response(
        &self,
        ctx: CoreContext,
        orig: UnbundleResponse,
    ) -> Result<UnbundleResponse, Error> {
        use UnbundleResponse::*;
        match orig {
            PushRebase(resp) => Ok(PushRebase(
                self.convert_unbundle_pushrebase_response(ctx, resp).await?,
            )),
            BookmarkOnlyPushRebase(resp) => Ok(BookmarkOnlyPushRebase(
                self.convert_unbundle_bookmark_only_pushrebase_response(ctx, resp)
                    .await?,
            )),
            Push(resp) => Ok(Push(self.convert_unbundle_push_response(ctx, resp).await?)),
            InfinitePush(resp) => Ok(InfinitePush(
                self.convert_unbundle_infinite_push_response(ctx, resp)
                    .await?,
            )),
        }
    }

    /// Convert `UnbundlePushRebaseResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_pushrebase_response(
        &self,
        ctx: CoreContext,
        orig: UnbundlePushRebaseResponse,
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
        backsync_all_latest(
            ctx.clone(),
            self.large_to_small_commit_syncer.clone(),
            self.target_repo_dbs.clone(),
        )
        .await?;

        let (pushrebased_rev, pushrebased_changesets) = try_join!(
            self.remap_changeset_expect_rewritten_or_preserved(
                ctx.clone(),
                &self.large_to_small_commit_syncer,
                pushrebased_rev,
            ),
            self.convert_pushrebased_changesets(ctx.clone(), pushrebased_changesets),
        )?;

        let onto = self
            .large_to_small_commit_syncer
            .rename_bookmark(&onto)
            .ok_or(format_err!(
                "bookmark_renamer unexpectedly dropped {} in {:?}",
                onto,
                self.large_to_small_commit_syncer
            ))?;

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
        ctx: CoreContext,
        orig: UnbundleBookmarkOnlyPushRebaseResponse,
    ) -> Result<UnbundleBookmarkOnlyPushRebaseResponse, Error> {
        // `UnbundleBookmarkOnlyPushRebaseResponse` consists of only one field:
        // `bookmark_push_part_id`, which does not need to be converted
        // We do, however, need to wait until the backsyncer catches up with
        // with the `bookmarks_update_log` tailing
        backsync_all_latest(
            ctx.clone(),
            self.large_to_small_commit_syncer.clone(),
            self.target_repo_dbs.clone(),
        )
        .await?;

        Ok(orig)
    }

    /// Convert `UnbundlePushResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_push_response(
        &self,
        ctx: CoreContext,
        orig: UnbundlePushResponse,
    ) -> Result<UnbundlePushResponse, Error> {
        // `UnbundlePushResponse` consists of only two fields:
        // `changegroup_id` and `bookmark_ids`, which do not need to be converted
        // We do, however, need to wait until the backsyncer catches up with
        // with the `bookmarks_update_log` tailing
        backsync_all_latest(
            ctx.clone(),
            self.large_to_small_commit_syncer.clone(),
            self.target_repo_dbs.clone(),
        )
        .await?;

        Ok(orig)
    }

    /// Convert `UnbundleInfinitePushResponse` struct in a large-to-small
    /// direction to be suitable for response generation in the small repo
    async fn convert_unbundle_infinite_push_response(
        &self,
        _ctx: CoreContext,
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
    async fn get_small_to_large_commit_equivalent(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<ChangesetId, Error> {
        self.remap_changeset_expect_rewritten_or_preserved(
            ctx,
            &self.small_to_large_commit_syncer,
            source_cs_id,
        )
        .await
    }

    /// Query the changeset mapping from the provided `syncer`
    /// Error out if the `CommitSyncOutcome` is not `RewrittenAs` or `Preserved`
    /// The logic of this method is to express an expectation that `cs_id`
    /// from the source repo MUST be properly present in the target repo,
    /// either with paths moved, or preserved. What is unacceptable is that
    /// the changeset is not yet synced, or rewritten into nothingness, or
    /// preserved from a different repo.
    async fn remap_changeset_expect_rewritten_or_preserved(
        &self,
        ctx: CoreContext,
        syncer: &CommitSyncer<Arc<dyn SyncedCommitMapping>>,
        cs_id: ChangesetId,
    ) -> Result<ChangesetId, Error> {
        let maybe_commit_sync_outcome = syncer.get_commit_sync_outcome(ctx.clone(), cs_id).await?;
        maybe_commit_sync_outcome
            .ok_or(format_err!(
                "Unexpected absence of CommitSyncOutcome for {} in {:?}",
                cs_id,
                syncer
            ))
            .and_then(|commit_sync_outcome| match commit_sync_outcome {
                CommitSyncOutcome::Preserved => Ok(cs_id),
                CommitSyncOutcome::RewrittenAs(rewritten) => Ok(rewritten),
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
        ctx: CoreContext,
        orig: InfiniteBookmarkPush<ChangesetId>,
    ) -> Result<InfiniteBookmarkPush<ChangesetId>, Error> {
        let maybe_old = orig.old.clone();
        let new = orig.new.clone();

        let (old, new) = try_join!(
            async {
                match maybe_old {
                    None => Ok(None),
                    Some(old) => self
                        .get_small_to_large_commit_equivalent(ctx.clone(), old)
                        .await
                        .map(Some),
                }
            },
            self.get_small_to_large_commit_equivalent(ctx.clone(), new),
        )?;

        Ok(InfiniteBookmarkPush { old, new, ..orig })
    }

    /// Convert `PlainBookmarkPush<ChangesetId>` in the small-to-large direction
    /// Note: this does not cause any changesets to be synced, just converts the struct
    ///       all the syncing is expected to be done prior to calling this fn.
    async fn convert_plain_bookmark_push_small_to_large(
        &self,
        ctx: CoreContext,
        orig: PlainBookmarkPush<ChangesetId>,
    ) -> Result<PlainBookmarkPush<ChangesetId>, Error> {
        let PlainBookmarkPush {
            part_id,
            name,
            old: maybe_old,
            new: maybe_new,
        } = orig;

        if self
            .commit_sync_config
            .common_pushrebase_bookmarks
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
                        .get_small_to_large_commit_equivalent(ctx.clone(), old)
                        .await
                        .map(Some),
                }
            },
            async {
                match maybe_new {
                    None => Ok(None),
                    Some(new) => self
                        .get_small_to_large_commit_equivalent(ctx.clone(), new)
                        .await
                        .map(Some),
                }
            },
        )?;

        let name = self
            .small_to_large_commit_syncer
            .rename_bookmark(&name)
            .ok_or(format_err!(
                "Bookmark {} unexpectedly dropped in {:?}",
                name,
                self.small_to_large_commit_syncer
            ))?;

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
        ctx: CoreContext,
        pushrebase_bookmark_spec: PushrebaseBookmarkSpec<ChangesetId>,
    ) -> Result<PushrebaseBookmarkSpec<ChangesetId>, Error> {
        match pushrebase_bookmark_spec {
            PushrebaseBookmarkSpec::NormalPushrebase(onto_params) => {
                let OntoBookmarkParams { bookmark } = onto_params;
                let bookmark = self
                    .small_to_large_commit_syncer
                    .rename_bookmark(&bookmark)
                    .ok_or(format_err!(
                        "Bookmark {} unexpectedly dropped in {:?}",
                        bookmark,
                        self.small_to_large_commit_syncer
                    ))?;

                Ok(PushrebaseBookmarkSpec::NormalPushrebase(
                    OntoBookmarkParams { bookmark },
                ))
            }
            PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => {
                let converted = self
                    .convert_plain_bookmark_push_small_to_large(ctx.clone(), plain_push)
                    .await?;
                Ok(PushrebaseBookmarkSpec::ForcePushrebase(converted))
            }
        }
    }

    /// Convert `PushrebaseChangesetPair` struct in the large-to-small direction
    async fn convert_pushrebase_changeset_pair(
        &self,
        ctx: CoreContext,
        pushrebase_changeset_pair: PushrebaseChangesetPair,
    ) -> Result<PushrebaseChangesetPair, Error> {
        let PushrebaseChangesetPair { id_old, id_new } = pushrebase_changeset_pair;
        let (id_old, id_new) = try_join!(
            self.remap_changeset_expect_rewritten_or_preserved(
                ctx.clone(),
                &self.large_to_small_commit_syncer,
                id_old
            ),
            self.remap_changeset_expect_rewritten_or_preserved(
                ctx.clone(),
                &self.large_to_small_commit_syncer,
                id_new
            ),
        )?;
        Ok(PushrebaseChangesetPair { id_old, id_new })
    }

    /// Convert all the produced `PushrebaseChangesetPair` structs in the
    /// large-to-small direction
    async fn convert_pushrebased_changesets(
        &self,
        ctx: CoreContext,
        pushrebased_changesets: Vec<PushrebaseChangesetPair>,
    ) -> Result<Vec<PushrebaseChangesetPair>, Error> {
        try_join_all(pushrebased_changesets.into_iter().map({
            |pushrebase_changeset_pair| {
                self.convert_pushrebase_changeset_pair(ctx.clone(), pushrebase_changeset_pair)
            }
        }))
        .await
    }

    /// Take changesets uploaded during the `unbundle` resolution
    /// and sync all the changesets into a large repo, while remembering which small cs id
    /// corresponds to which large cs id
    async fn sync_uploaded_changesets(
        &self,
        ctx: CoreContext,
        uploaded_map: UploadedBonsais,
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
            .ok_or(format_err!("Cycle in the uploaded changeset DAG!"))?
            .into_iter()
            .rev()
            .collect();

        let mut synced_ids = Vec::new();

        for bcs_id in to_sync.iter() {
            let synced_bcs_id = self
                .small_to_large_commit_syncer
                .sync_commit(ctx.clone(), *bcs_id)
                .await?
                .ok_or(format_err!(
                    "{} was rewritten into nothingness during uploaded changesets sync",
                    bcs_id
                ))?;
            synced_ids.push((bcs_id, synced_bcs_id));
        }

        try_join_all(
            synced_ids
                .into_iter()
                .map(move |(small_bcs_id, target_repo_bcs_id)| {
                    cloned!(ctx, target_repo);
                    async move {
                        let target_bcs = target_repo
                            .get_bonsai_changeset(ctx, target_repo_bcs_id)
                            .compat()
                            .await?;

                        Ok((*small_bcs_id, target_bcs))
                    }
                }),
        )
        .await
        .map(|v| v.into_iter().collect())
    }
}
