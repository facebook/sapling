/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use blobrepo::BlobRepo;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data::batch::split_batch_in_linear_stacks;
use derived_data::{derive_impl, BonsaiDerivedMapping};
use futures::stream::{FuturesOrdered, TryStreamExt};
use mononoke_types::{ChangesetId, FsnodeId};

use crate::derive::derive_fsnode;
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
pub async fn derive_fsnode_in_batch<Mapping>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mapping: &Mapping,
    batch: Vec<ChangesetId>,
    gap_size: Option<usize>,
) -> Result<HashMap<ChangesetId, FsnodeId>, Error>
where
    Mapping: BonsaiDerivedMapping<Value = RootFsnodeId> + 'static,
{
    let linear_stacks = split_batch_in_linear_stacks(ctx, repo, batch).await?;
    let mut res = HashMap::new();
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
                    match res.get(&p) {
                        Some(fsnode_id) => Ok::<_, Error>(*fsnode_id),
                        None => Ok(derive_impl::derive_impl::<RootFsnodeId, Mapping>(
                            ctx, repo, mapping, p,
                        )
                        .await?
                        .into_fsnode_id()),
                    }
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        let to_derive = match gap_size {
            Some(gap_size) => linear_stack
                .file_changes
                .chunks(gap_size)
                .filter_map(|chunk| chunk.last().cloned())
                .collect(),
            None => linear_stack.file_changes,
        };

        let new_fsnodes = to_derive
            .into_iter()
            .map(|(cs_id, fc)| {
                // Clone the values that we need owned copies of to move
                // into the future we are going to spawn, which means it
                // must have static lifetime.
                cloned!(ctx, repo, parent_fsnodes);
                async move {
                    let derivation_fut = async move {
                        derive_fsnode(&ctx, &repo, parent_fsnodes, fc.into_iter().collect()).await
                    };
                    let derivation_handle = tokio::spawn(derivation_fut);
                    let fsnode_id: FsnodeId = derivation_handle.await??;
                    Result::<_, Error>::Ok((cs_id, fsnode_id))
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        res.extend(new_fsnodes);
    }

    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;
    use derived_data::BonsaiDerived;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures::compat::Stream01CompatExt;
    use revset::AncestorsNodeStream;
    use tests_utils::resolve_cs_id;

    #[fbinit::test]
    async fn batch_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let batch = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

            let mapping = RootFsnodeId::default_mapping(&ctx, &repo)?;
            let mut cs_ids =
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                    .compat()
                    .try_collect::<Vec<_>>()
                    .await?;
            cs_ids.reverse();
            let fsnode_ids = derive_fsnode_in_batch(&ctx, &repo, &mapping, cs_ids, None).await?;
            fsnode_ids.get(&master_cs_id).unwrap().clone()
        };

        let sequential = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            RootFsnodeId::derive(&ctx, &repo, master_cs_id)
                .await?
                .into_fsnode_id()
        };

        assert_eq!(batch, sequential);
        Ok(())
    }
}
