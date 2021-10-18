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
use mononoke_types::{ChangesetId, FsnodeId};
use tunables::tunables;

use crate::derive::{derive_fsnode, derive_fsnodes_stack};
use crate::RootFsnodeId;

/// Derive a batch of fsnodes, potentially doing it faster than deriving fsnodes sequentially.
/// The primary purpose of this is to be used while backfilling fsnodes for a large repository.
///
/// The best results are achieved if a batch is a linear stack (i.e. no merges) of commits where batch[i-1] is a parent
/// of batch[i]. However if it's not the case then using derive_fsnode_in_batch shouldn't be much slower than a sequential
/// derivation of the same commits.
///
/// `derive_fsnode_in_batch` proceed in a few stages:
/// 1) Split `batch` in a a few linear stacks (there are certain rules about how it can be done, see `split_batch_in_linear_stacks` for more details)
/// 2) Stacks are processed one after another (i.e. we get benefits from parallel execution only if two commits are in the same stack)
/// 3) For each commit stack derive fsnode commits in parallel. This is done by calling `derive_fsnode()`
///    with parents of the first commit in the stack, and all bonsai file changes since first commit in the stack. See example below:
///
///   Stack:
///     Commit 1 - Added "file1" with content "A", has parent commit 0
///     Commit 2 - Added "file2" with content "B", has parent commit 1
///     Commit 3 - Modified "file1" with content "C", has parent commit 2
///
///   We make three derive_fsnode() calls in parallel with these parameters:
///      derive_fsnode([commit0], {"file1" => "A"})
///      derive_fsnode([commit0], {"file1" => "A", "file2" => "B"})
///      derive_fsnode([commit0], {"file1" => "C", "file2" => "B"})
///
/// So effectively we combine the changes from all commits in the stack. Note that it's not possible to do it for unodes
/// because unodes depend on the order of the changes.
///
/// Fsnode derivation can be cpu-bounded, and the speed up is achieved by spawning derivation on different
/// tokio tasks - this allows us to use more cpu.
pub async fn derive_fsnode_in_batch(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    batch: Vec<ChangesetId>,
    gap_size: Option<usize>,
) -> Result<HashMap<ChangesetId, RootFsnodeId>, Error> {
    let linear_stacks = split_batch_in_linear_stacks(
        ctx,
        derivation_ctx.blobstore(),
        batch,
        FileConflicts::ChangeDelete,
    )
    .await?;
    let mut res: HashMap<ChangesetId, RootFsnodeId> = HashMap::new();
    for linear_stack in linear_stacks {
        // Fetch the parent fsnodes, either from a previous iteration of this
        // loop (which will have stored the mapping in `res`), or from the
        // main mapping, where they should already be derived.
        let parent_fsnodes = linear_stack
            .parents
            .into_iter()
            .map(|p| {
                borrowed!(res);
                async move {
                    anyhow::Result::<_>::Ok(
                        match res.get(&p) {
                            Some(fsnode_id) => fsnode_id.clone(),
                            None => derivation_ctx.fetch_dependency(ctx, p).await?,
                        }
                        .into_fsnode_id(),
                    )
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        let new_fsnodes =
            if !tunables().get_fsnodes_use_new_batch_derivation() || gap_size.is_some() {
                old_batch_derivation(
                    ctx,
                    derivation_ctx,
                    parent_fsnodes,
                    gap_size,
                    linear_stack.file_changes,
                )
                .await?
            } else {
                new_batch_derivation(
                    ctx,
                    derivation_ctx,
                    parent_fsnodes,
                    linear_stack.file_changes,
                )
                .await?
            };
        res.extend(new_fsnodes);
    }

    Ok(res)
}

pub async fn old_batch_derivation(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    parent_fsnodes: Vec<FsnodeId>,
    gap_size: Option<usize>,
    file_changes: Vec<StackItem>,
) -> Result<Vec<(ChangesetId, RootFsnodeId)>, Error> {
    let to_derive = match gap_size {
        Some(gap_size) => file_changes
            .chunks(gap_size)
            .filter_map(|chunk| chunk.last().cloned())
            .collect(),
        None => file_changes,
    };

    let new_fsnodes = to_derive
        .into_iter()
        .map(|item| {
            // Clone the values that we need owned copies of to move
            // into the future we are going to spawn, which means it
            // must have static lifetime.
            cloned!(ctx, derivation_ctx, parent_fsnodes);
            async move {
                let cs_id = item.cs_id;
                let derivation_fut = async move {
                    derive_fsnode(
                        &ctx,
                        &derivation_ctx,
                        parent_fsnodes,
                        item.combined_file_changes.into_iter().collect(),
                    )
                    .await
                };
                let derivation_handle = tokio::spawn(derivation_fut);
                let fsnode_id = RootFsnodeId(derivation_handle.await??);
                anyhow::Result::<_>::Ok((cs_id, fsnode_id))
            }
        })
        .collect::<FuturesOrdered<_>>()
        .try_collect::<Vec<_>>()
        .await?;

    Ok(new_fsnodes)
}

pub async fn new_batch_derivation(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    parent_fsnode_manifests: Vec<FsnodeId>,
    file_changes: Vec<StackItem>,
) -> Result<Vec<(ChangesetId, RootFsnodeId)>, Error> {
    let mut res = HashMap::new();
    if parent_fsnode_manifests.len() > 1 {
        // we can't derive stack for a merge commit,
        // so let's derive it without batching
        for item in file_changes {
            let bonsai = item.cs_id.load(&ctx, derivation_ctx.blobstore()).await?;
            let parents = derivation_ctx
                .fetch_unknown_parents(ctx, Some(&res), &bonsai)
                .await?;
            let derived = RootFsnodeId::derive_single(ctx, derivation_ctx, bonsai, parents).await?;
            res.insert(item.cs_id, derived);
        }
    } else {
        let first = file_changes.first().map(|item| item.cs_id);
        let last = file_changes.last().map(|item| item.cs_id);

        let file_changes: Vec<_> = file_changes
            .into_iter()
            .map(|item| (item.cs_id, item.per_commit_file_changes))
            .collect();

        let derived = derive_fsnodes_stack(
            ctx,
            derivation_ctx,
            file_changes,
            parent_fsnode_manifests.get(0).map(|mf_id| *mf_id),
        )
        .await
        .with_context(|| format!("failed deriving stack of {:?} to {:?}", first, last,))?;

        res.extend(
            derived
                .into_iter()
                .map(|(csid, mf_id)| (csid, RootFsnodeId(mf_id))),
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
                .backfill_batch::<RootFsnodeId>(
                    &ctx,
                    cs_ids,
                    BatchDeriveOptions::Parallel { gap_size: None },
                    None,
                )
                .await?;
            manager
                .fetch_derived::<RootFsnodeId>(&ctx, master_cs_id, None)
                .await?
                .unwrap()
                .into_fsnode_id()
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
                    .backfill_batch::<RootFsnodeId>(
                        &ctx,
                        cs_ids,
                        BatchDeriveOptions::Parallel { gap_size: None },
                        None,
                    )
                    .boxed(),
            )
            .await?;

            manager
                .fetch_derived::<RootFsnodeId>(&ctx, master_cs_id, None)
                .await?
                .unwrap()
                .into_fsnode_id()
        };

        let sequential = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            repo.repo_derived_data()
                .manager()
                .derive::<RootFsnodeId>(&ctx, master_cs_id, None)
                .await?
                .into_fsnode_id()
        };

        assert_eq!(old_batch, sequential);
        assert_eq!(new_batch, sequential);
        Ok(())
    }
}
