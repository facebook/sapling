/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{Context, Error};
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data::batch::{split_batch_in_linear_stacks, FileConflicts, StackItem};
use derived_data_manager::{BonsaiDerivable, DerivationContext};
use futures::stream::{FuturesOrdered, TryStreamExt};
use mononoke_types::ChangesetId;
use tunables::tunables;

use crate::derive::{derive_skeleton_manifest, derive_skeleton_manifest_stack};
use crate::{RootSkeletonManifestId, SkeletonManifestId};

/// Derive a batch of skeleton manifests, potentially doing it faster than
/// deriving skeleton manifests sequentially.  The primary purpose of this is
/// to be used while backfilling skeleton manifests for a large repository.
///
/// This is the same mechanism as fsnodes, see `derive_fsnode_in_batch` for
/// more details.
pub async fn derive_skeleton_manifests_in_batch(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    batch: Vec<ChangesetId>,
    gap_size: Option<usize>,
) -> Result<HashMap<ChangesetId, RootSkeletonManifestId>, Error> {
    let linear_stacks = split_batch_in_linear_stacks(
        ctx,
        derivation_ctx.blobstore(),
        batch,
        FileConflicts::ChangeDelete,
    )
    .await?;
    let mut res: HashMap<ChangesetId, RootSkeletonManifestId> = HashMap::new();
    for linear_stack in linear_stacks {
        // Fetch the parent skeleton manifests, either from a previous
        // iteration of this loop (which will have stored the mapping in
        // `res`, or from the main mapping, where they should already be
        // derived.
        let parent_skeleton_manifests = linear_stack
            .parents
            .into_iter()
            .map(|p| {
                borrowed!(res);
                async move {
                    anyhow::Result::<_>::Ok(
                        match res.get(&p) {
                            Some(sk_mf_id) => sk_mf_id.clone(),
                            None => derivation_ctx.fetch_dependency(ctx, p).await?,
                        }
                        .into_skeleton_manifest_id(),
                    )
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        let new_skeleton_manifests = if !tunables()
            .get_skeleton_manifests_use_new_batch_derivation()
            || gap_size.is_some()
        {
            old_batch_derivation(
                ctx,
                derivation_ctx,
                parent_skeleton_manifests,
                gap_size,
                linear_stack.file_changes,
            )
            .await?
        } else {
            new_batch_derivation(
                ctx,
                derivation_ctx,
                parent_skeleton_manifests,
                linear_stack.file_changes,
            )
            .await?
        };
        res.extend(new_skeleton_manifests);
    }

    Ok(res)
}

pub async fn old_batch_derivation(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    parent_skeleton_manifests: Vec<SkeletonManifestId>,
    gap_size: Option<usize>,
    file_changes: Vec<StackItem>,
) -> Result<Vec<(ChangesetId, RootSkeletonManifestId)>, Error> {
    let to_derive = match gap_size {
        Some(gap_size) => file_changes
            .chunks(gap_size)
            .filter_map(|chunk| chunk.last().cloned())
            .collect(),
        None => file_changes,
    };

    let new_skeleton_manifests = to_derive
        .into_iter()
        .map(|item| {
            // Clone the values that we need owned copies of to move
            // into the future we are going to spawn, which means it
            // must have static lifetime.
            cloned!(ctx, derivation_ctx, parent_skeleton_manifests);
            async move {
                let cs_id = item.cs_id;
                let derivation_fut = async move {
                    derive_skeleton_manifest(
                        &ctx,
                        &derivation_ctx,
                        parent_skeleton_manifests,
                        item.combined_file_changes.into_iter().collect(),
                    )
                    .await
                };
                let derivation_handle = tokio::spawn(derivation_fut);
                let sk_mf_id = RootSkeletonManifestId(derivation_handle.await??);
                Result::<_, Error>::Ok((cs_id, sk_mf_id))
            }
        })
        .collect::<FuturesOrdered<_>>()
        .try_collect::<Vec<_>>()
        .await?;

    Ok(new_skeleton_manifests)
}

pub async fn new_batch_derivation(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    parent_skeleton_manifests: Vec<SkeletonManifestId>,
    file_changes: Vec<StackItem>,
) -> Result<Vec<(ChangesetId, RootSkeletonManifestId)>, Error> {
    let mut res = HashMap::new();
    if parent_skeleton_manifests.len() > 1 {
        // we can't derive stack for a merge commit,
        // so let's derive it without batching
        for item in file_changes {
            let bonsai = item.cs_id.load(&ctx, derivation_ctx.blobstore()).await?;
            let parents = derivation_ctx
                .fetch_unknown_parents(ctx, Some(&res), &bonsai)
                .await?;
            let derived =
                RootSkeletonManifestId::derive_single(ctx, derivation_ctx, bonsai, parents).await?;
            res.insert(item.cs_id, derived);
        }
    } else {
        let first = file_changes.first().map(|item| item.cs_id);
        let last = file_changes.last().map(|item| item.cs_id);
        let derived = derive_skeleton_manifest_stack(
            ctx,
            derivation_ctx,
            file_changes
                .into_iter()
                .map(|item| (item.cs_id, item.per_commit_file_changes))
                .collect(),
            parent_skeleton_manifests.get(0).map(|mf_id| *mf_id),
        )
        .await
        .with_context(|| format!("failed deriving stack of {:?} to {:?}", first, last,))?;

        res.extend(
            derived
                .into_iter()
                .map(|(csid, mf_id)| (csid, RootSkeletonManifestId(mf_id))),
        );
    }

    Ok(res.into_iter().collect())
}

#[cfg(test)]
mod test {
    use super::*;
    use derived_data_manager::BatchDeriveOptions;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures::{compat::Stream01CompatExt, FutureExt};
    use maplit::hashmap;
    use repo_derived_data::RepoDerivedDataRef;
    use revset::AncestorsNodeStream;
    use tests_utils::resolve_cs_id;
    use tunables::{with_tunables_async, MononokeTunables};

    #[fbinit::test]
    async fn batch_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let old_batch = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

            let mut cs_ids =
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                    .compat()
                    .try_collect::<Vec<_>>()
                    .await?;
            cs_ids.reverse();
            let manager = repo.repo_derived_data().manager();
            manager
                .backfill_batch::<RootSkeletonManifestId>(
                    &ctx,
                    cs_ids,
                    BatchDeriveOptions::Parallel { gap_size: None },
                    None,
                )
                .await?;
            manager
                .fetch_derived::<RootSkeletonManifestId>(&ctx, master_cs_id, None)
                .await?
                .unwrap()
                .into_skeleton_manifest_id()
        };

        let new_batch = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

            let mut cs_ids =
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                    .compat()
                    .try_collect::<Vec<_>>()
                    .await?;
            cs_ids.reverse();
            let manager = repo.repo_derived_data().manager();

            let tunables = MononokeTunables::default();
            tunables.update_bools(&hashmap! {
                "skeleton_manifests_use_new_batch_derivation".to_string() => true,
            });

            with_tunables_async(
                tunables,
                manager
                    .backfill_batch::<RootSkeletonManifestId>(
                        &ctx,
                        cs_ids,
                        BatchDeriveOptions::Parallel { gap_size: None },
                        None,
                    )
                    .boxed(),
            )
            .await?;
            manager
                .fetch_derived::<RootSkeletonManifestId>(&ctx, master_cs_id, None)
                .await?
                .unwrap()
                .into_skeleton_manifest_id()
        };

        let sequential = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            repo.repo_derived_data()
                .manager()
                .derive::<RootSkeletonManifestId>(&ctx, master_cs_id, None)
                .await?
                .into_skeleton_manifest_id()
        };

        assert_eq!(old_batch, sequential);
        assert_eq!(new_batch, sequential);
        Ok(())
    }
}
