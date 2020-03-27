/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{format_err, Result};
use maplit::hashset;

use dag::Id as SegmentedChangelogId;

use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::parents::Parents;

#[derive(Debug)]
pub struct IdMap {
    next_scid: u64,
    name_to_scid: HashMap<ChangesetId, SegmentedChangelogId>,
    scid_to_name: HashMap<SegmentedChangelogId, ChangesetId>,
}

impl IdMap {
    pub fn new() -> Self {
        IdMap {
            next_scid: dag::Group::MASTER.min_id().0,
            name_to_scid: HashMap::new(),
            scid_to_name: HashMap::new(),
        }
    }
}

// TODO(sfilip): these will have to be async
impl IdMap {
    pub fn find_scid_by_name(&self, name: &ChangesetId) -> Result<Option<SegmentedChangelogId>> {
        Ok(self.name_to_scid.get(name).map(|scid| *scid))
    }

    pub fn convert_name(&self, name: &ChangesetId) -> Result<SegmentedChangelogId> {
        self.find_scid_by_name(name)?
            .ok_or_else(|| format_err!("Failed to find find changeset id {} in IdMap", name))
    }

    // TODO(sfilip): these will have to be async
    pub fn find_name_by_scid(&self, scid: &SegmentedChangelogId) -> Result<Option<ChangesetId>> {
        Ok(self.scid_to_name.get(scid).map(|name| *name))
    }

    pub fn convert_scid(&self, scid: &SegmentedChangelogId) -> Result<ChangesetId> {
        self.find_name_by_scid(&scid)?
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", scid))
    }
}

// NOTE. Test method. IdMap will be built by an external process.
impl IdMap {
    pub async fn build_up(
        &mut self,
        ctx: &CoreContext,
        blob_repo: &BlobRepo,
        head: ChangesetId,
    ) -> Result<SegmentedChangelogId> {
        enum Todo {
            Visit(ChangesetId),
            Assign(ChangesetId),
        }
        let parents = Parents::new(ctx, blob_repo);
        let mut todo_stack = vec![Todo::Visit(head)];
        let mut seen = hashset![head];
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
                    self.insert(changeset_id, SegmentedChangelogId(self.next_scid));
                    self.next_scid += 1;
                }
            }
        }
        match self.name_to_scid.get(&head) {
            None => Err(format_err!(
                "Error building IdMap. Failed to assign head {}",
                head
            )),
            Some(sc_id) => Ok(*sc_id),
        }
    }

    fn insert(&mut self, name: ChangesetId, scid: SegmentedChangelogId) {
        self.name_to_scid.insert(name, scid);
        self.scid_to_name.insert(scid, name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;
    use futures::compat::{Future01CompatExt, Stream01CompatExt};
    use futures::StreamExt;
    use futures_old::stream::{self, Stream};
    use revset::AncestorsNodeStream;

    use fixtures::{linear, merge_even, merge_uneven};

    async fn assert_topologic_sorted(
        ctx: &CoreContext,
        blob_repo: &BlobRepo,
        head: ChangesetId,
        idmap: &IdMap,
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
                let parent_scid = idmap.find_scid_by_name(&parent).unwrap().unwrap();
                let changeset_scid = idmap.find_scid_by_name(&changeset_id).unwrap().unwrap();
                assert!(parent_scid < changeset_scid);
            }
        }
        Ok(())
    }

    #[fbinit::test]
    fn test_build_idmap_linear(fb: FacebookInit) -> Result<()> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on_std(async move {
            let ctx = CoreContext::test_mock(fb);
            let repo = linear::getrepo(fb).await;

            let head = ChangesetId::from_str(
                "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6",
            )?;
            let mut idmap = IdMap::new();
            idmap.build_up(&ctx, &repo, head).await?;
            assert_topologic_sorted(&ctx, &repo, head, &idmap).await?;

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_build_idmap_merge_even(fb: FacebookInit) -> Result<()> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on_std(async move {
            let ctx = CoreContext::test_mock(fb);
            let repo = merge_even::getrepo(fb).await;

            let head = ChangesetId::from_str(
                "567a25d453cafaef6550de955c52b91bf9295faf38d67b6421d5d2e532e5adef",
            )?;
            let mut idmap = IdMap::new();
            idmap.build_up(&ctx, &repo, head).await?;
            assert_topologic_sorted(&ctx, &repo, head, &idmap).await?;

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_build_idmap_merge_uneven(fb: FacebookInit) -> Result<()> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on_std(async move {
            let ctx = CoreContext::test_mock(fb);
            let repo = merge_uneven::getrepo(fb).await;

            let head = ChangesetId::from_str(
                "288d72de7fd26ebcd19f5e4f1b41542f22f4a9f7e2f6845fa04e8fd70064973d",
            )?;
            let mut idmap = IdMap::new();
            idmap.build_up(&ctx, &repo, head).await?;
            assert_topologic_sorted(&ctx, &repo, head, &idmap).await?;

            Ok(())
        })
    }
}
