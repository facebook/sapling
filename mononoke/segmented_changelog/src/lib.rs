/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![feature(backtrace)]
#![deny(warnings)]

///! segmented_changelog
///!
///! Data structures and algorithms for a commit graph used by source control.
use std::collections::HashMap;

use anyhow::Result;
use futures_preview::compat::Future01CompatExt;
use maplit::{hashmap, hashset};

use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;

/// An identifier in Segmented Changelog
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct SegmentedChangelogId(u64);

/// Assign an id for a head in a DAG. This implies ancestors of the
/// head will also have ids assigned.
pub async fn build_idmap(
    ctx: &CoreContext,
    blob_repo: &BlobRepo,
    head: ChangesetId,
) -> Result<HashMap<ChangesetId, SegmentedChangelogId>> {
    struct Parents<'a> {
        ctx: &'a CoreContext,
        blob_repo: &'a BlobRepo,
    }
    impl<'a> Parents<'a> {
        async fn get(&self, changeset_id: ChangesetId) -> Result<Vec<ChangesetId>> {
            let parents = self
                .blob_repo
                .get_changeset_parents_by_bonsai(self.ctx.clone(), changeset_id)
                .compat()
                .await?;
            Ok(parents)
        }
    }
    enum Todo {
        Visit(ChangesetId),
        Assign(ChangesetId),
    }
    let parents = Parents { ctx, blob_repo };
    let mut todo_stack = vec![Todo::Visit(head)];
    let mut seen = hashset![head];
    let mut idmap = hashmap![];
    let mut next_segmented_changelog_id = 1;

    while let Some(todo) = todo_stack.pop() {
        match todo {
            Todo::Visit(changeset_id) => {
                todo_stack.push(Todo::Assign(changeset_id));

                let parents = parents.get(changeset_id).await?;
                for parent in parents.into_iter().rev() {
                    // Note: iterating parents in reverse is a small optimization because in our setup p1 is master.
                    if !seen.contains(&parent) {
                        seen.insert(parent);
                        todo_stack.push(Todo::Visit(parent));
                    }
                }
            }
            Todo::Assign(changeset_id) => {
                idmap.insert(
                    changeset_id,
                    SegmentedChangelogId(next_segmented_changelog_id),
                );
                next_segmented_changelog_id += 1;
            }
        }
    }

    Ok(idmap)
}

// TODO(sfilip):
// generate_graph
// struct Dag(idmap, segments)

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;
    use futures::stream::{self, Stream};
    use futures_preview::StreamExt;
    use futures_preview::{FutureExt as NewFutureExt, TryFutureExt};
    use futures_util::compat::Stream01CompatExt;
    use revset::AncestorsNodeStream;

    use fixtures::{linear, merge_even, merge_uneven};

    async fn assert_topologic_sorted(
        ctx: &CoreContext,
        blob_repo: &BlobRepo,
        head: ChangesetId,
        idmap: HashMap<ChangesetId, SegmentedChangelogId>,
    ) -> Result<()> {
        // It's a bit weird to use the ancestors stream. The idea is just to do
        // something different than the core implementation.
        let ancestors =
            AncestorsNodeStream::new(ctx.clone(), &blob_repo.get_changeset_fetcher(), head);
        let mut to_check = stream::iter_result(vec![Ok(head)])
            .chain(ancestors)
            .compat();
        while let Some(changeset_id) = to_check.next().await {
            let changeset_id = changeset_id?;
            let parents = blob_repo
                .get_changeset_parents_by_bonsai(ctx.clone(), changeset_id)
                .compat()
                .await?;
            for parent in parents {
                assert!(idmap.get(&parent).unwrap() < idmap.get(&changeset_id).unwrap());
            }
        }
        Ok(())
    }

    #[fbinit::test]
    fn test_build_idmap_linear(fb: FacebookInit) -> Result<()> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = linear::getrepo(fb);

                let head = ChangesetId::from_str(
                    "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6",
                )?;
                let idmap = build_idmap(&ctx, &repo, head).await?;
                assert_topologic_sorted(&ctx, &repo, head, idmap).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn test_build_idmap_merge_even(fb: FacebookInit) -> Result<()> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = merge_even::getrepo(fb);

                let head = ChangesetId::from_str(
                    "567a25d453cafaef6550de955c52b91bf9295faf38d67b6421d5d2e532e5adef",
                )?;
                let idmap = build_idmap(&ctx, &repo, head).await?;
                assert_topologic_sorted(&ctx, &repo, head, idmap).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }

    #[fbinit::test]
    fn test_build_idmap_merge_uneven(fb: FacebookInit) -> Result<()> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on(
            async move {
                let ctx = CoreContext::test_mock(fb);
                let repo = merge_uneven::getrepo(fb);

                let head = ChangesetId::from_str(
                    "288d72de7fd26ebcd19f5e4f1b41542f22f4a9f7e2f6845fa04e8fd70064973d",
                )?;
                let idmap = build_idmap(&ctx, &repo, head).await?;
                assert_topologic_sorted(&ctx, &repo, head, idmap).await?;

                Ok(())
            }
                .boxed()
                .compat(),
        )
    }
}
