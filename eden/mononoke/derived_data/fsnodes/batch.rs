/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, HashMap};

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{compat::Future01CompatExt, stream, stream::FuturesOrdered, StreamExt, TryStreamExt};
use itertools::Itertools;
use mononoke_types::{BonsaiChangeset, FileChange, MPath};
use mononoke_types::{ChangesetId, ContentId, FileType, FsnodeId};

use crate::derive::derive_fsnode;
use crate::RootFsnodeId;

type FileToContent = BTreeMap<MPath, Option<(ContentId, FileType)>>;

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
    repo: &BlobRepo,
    batch: Vec<ChangesetId>,
) -> Result<HashMap<ChangesetId, FsnodeId>, Error> {
    let linear_stacks = split_batch_in_linear_stacks(ctx, repo, batch).await?;
    let mut res = HashMap::new();
    for linear_stack in linear_stacks {
        let parent_fsnodes = linear_stack
            .parents
            .into_iter()
            .map(|p| RootFsnodeId::derive(ctx.clone(), repo.clone(), p).compat())
            .collect::<FuturesOrdered<_>>()
            .map_ok(|root_fsnode_id| root_fsnode_id.into_fsnode_id())
            .try_collect::<Vec<_>>()
            .await?;

        let new_fsnodes = linear_stack
            .file_changes
            .into_iter()
            .map(|(cs_id, fc)| {
                let parent_fsnodes = parent_fsnodes.clone();
                async move {
                    let f = derive_fsnode(
                        ctx.clone(),
                        repo.clone(),
                        parent_fsnodes,
                        fc.into_iter().collect(),
                    )
                    .compat();
                    let f = tokio::spawn(f);
                    let fsnode_id: FsnodeId = f.await??;
                    Result::<_, Error>::Ok((cs_id, fsnode_id))
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect::<Vec<_>>()
            .await?;

        // TODO(stash): update mapping here since fsnodes were already derived
        res.extend(new_fsnodes);
    }

    Ok(res)
}

/// We follow a few rules when splitting a batch in the stacks:
/// 1) Merges go to a separate batch
/// 2) If two commits have two files where one is a prefix of another, then they
///    go to a separate stacks (because of the way bonsai interprets these files)
/// 3) If a file was modified in one commit and delete in other then they go
///    to different stacks
async fn split_batch_in_linear_stacks(
    ctx: &CoreContext,
    repo: &BlobRepo,
    batch: Vec<ChangesetId>,
) -> Result<Vec<LinearStack>, Error> {
    let bonsais = stream::iter(batch.into_iter().map(|bcs_id| async move {
        let bcs = bcs_id
            .load(ctx.clone(), &repo.get_blobstore())
            .compat()
            .await?;
        Result::<_, Error>::Ok((bcs_id, bcs))
    }))
    .buffered(100)
    .try_collect::<Vec<_>>()
    .await?;

    let mut bonsai_iter = bonsais.into_iter().peekable();
    let (start_bcs_id, start_bcs) = match bonsai_iter.peek() {
        Some(val) => val,
        None => {
            return Ok(vec![]);
        }
    };

    let mut linear_stacks = vec![];
    let mut cur_linear_stack = LinearStack::new(start_bcs.parents().collect::<Vec<_>>());
    cur_linear_stack.push(*start_bcs_id, start_bcs);

    for (prev_cs, (bcs_id, bcs)) in bonsai_iter.tuple_windows() {
        if !cur_linear_stack.can_be_in_same_linear_stack(&prev_cs, &bcs) {
            linear_stacks.push(cur_linear_stack);
            cur_linear_stack = LinearStack::new(bcs.parents().collect::<Vec<_>>());
        }

        cur_linear_stack.push(bcs_id, &bcs);
    }

    linear_stacks.push(cur_linear_stack);

    Ok(linear_stacks)
}

// Stores a linear stack.
// `file_changes` contains the list of file changes that need to be applied to `parents`
// to generate fsnodes for a particular commit.
struct LinearStack {
    parents: Vec<ChangesetId>,
    file_changes: Vec<(ChangesetId, FileToContent)>,
}

impl LinearStack {
    fn new(parents: Vec<ChangesetId>) -> Self {
        Self {
            parents,
            file_changes: vec![],
        }
    }

    fn push(&mut self, cs_id: ChangesetId, bcs: &BonsaiChangeset) {
        let mut fc = self.get_last_file_changes().cloned().unwrap_or_default();
        fc.extend(bcs.file_changes().map(|(path, maybe_fc)| {
            (
                path.clone(),
                maybe_fc.map(|fc| (fc.content_id(), fc.file_type())),
            )
        }));
        self.file_changes.push((cs_id, fc));
    }

    fn can_be_in_same_linear_stack(
        &self,
        (prev_cs_id, prev): &(ChangesetId, BonsaiChangeset),
        next: &BonsaiChangeset,
    ) -> bool {
        // Each merge should go in a separate stack
        if prev.is_merge() || next.is_merge() {
            return false;
        }

        if next.parents().find(|p| p == prev_cs_id).is_none() {
            return false;
        }

        if let Some(cur_file_changes) = self.get_last_file_changes() {
            let new_file_changes = next.file_changes().collect();
            if has_file_conflict(cur_file_changes, new_file_changes) {
                return false;
            }
        }

        return true;
    }

    fn get_last_file_changes(&self) -> Option<&FileToContent> {
        self.file_changes.last().map(|(_, fc)| fc)
    }
}

/// Returns true if:
/// 1) any of the files in `left` are prefix of any file in `right` and vice-versa
/// 2) File with the same name was deleted in `left` and added in `right` or vice-versa
fn has_file_conflict(left: &FileToContent, right: BTreeMap<&MPath, Option<&FileChange>>) -> bool {
    let mut left = left.iter();
    let mut right = right.into_iter();
    let mut state = (left.next(), right.next());
    loop {
        state = match state {
            (Some((l_path, l_content)), Some((r_path, r_file_change))) => match l_path.cmp(&r_path)
            {
                CmpOrdering::Equal => {
                    if l_content.is_some() != r_file_change.is_some() {
                        // File is deleted and modified in two different commits -
                        // this is a conflict, they can't be in the same stack
                        return true;
                    }
                    (left.next(), right.next())
                }
                CmpOrdering::Less => {
                    if l_path.is_prefix_of(r_path) {
                        return true;
                    }
                    (left.next(), Some((r_path, r_file_change)))
                }
                CmpOrdering::Greater => {
                    if r_path.is_prefix_of(l_path) {
                        return true;
                    }
                    (Some((l_path, l_content)), right.next())
                }
            },
            _ => break,
        };
    }

    false
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures::compat::Stream01CompatExt;
    use maplit::btreemap;
    use mononoke_types::FileType;
    use revset::AncestorsNodeStream;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    #[fbinit::compat_test]
    async fn test_split_batch_in_linear_stacks_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::TestRepoBuilder::new().build()?;

        let file1 = "file1";
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file1, "content1")
            .commit()
            .await?;
        let file2 = "file2";
        let second = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file(file2, "content2")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![root]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![(
                vec![],
                vec![btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))}],
            )],
        )
        .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![second]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![(
                vec![root],
                vec![btreemap! {file2 => Some(("content2".to_string(), FileType::Regular))}],
            )],
        )
        .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![root, second]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![(
                vec![],
                vec![
                    btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))},
                    btreemap! {
                        file1 => Some(("content1".to_string(), FileType::Regular)),
                        file2 => Some(("content2".to_string(), FileType::Regular)),
                    },
                ],
            )],
        )
        .await?;

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_split_batch_in_linear_stacks_merge(fb: FacebookInit) -> Result<(), Error> {
        let repo = blobrepo_factory::TestRepoBuilder::new().build()?;
        let ctx = CoreContext::test_mock(fb);

        let file1 = "file1";
        let p1 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file1, "content1")
            .commit()
            .await?;
        let file2 = "file2";
        let p2 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file2, "content2")
            .commit()
            .await?;
        let merge_file = "merge";
        let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
            .add_file(merge_file, "merge")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![p1, merge]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![
                (
                    vec![],
                    vec![btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (
                    vec![p1, p2],
                    vec![btreemap! {merge_file => Some(("merge".to_string(), FileType::Regular))}],
                ),
            ],
        )
        .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![p1, p2]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![
                (
                    vec![],
                    vec![btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (
                    vec![],
                    vec![btreemap! {file2 => Some(("content2".to_string(), FileType::Regular))}],
                ),
            ],
        )
        .await?;

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_split_batch_in_linear_stacks_replace_dir_with_file(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = blobrepo_factory::TestRepoBuilder::new().build()?;
        let ctx = CoreContext::test_mock(fb);

        let dir = "dir";
        let file1 = format!("{}/file1", dir);
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(&file1, "content1")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file(dir, "replace_dir_with_file")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![root, child]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![
                (
                    vec![],
                    vec![btreemap! {file1.as_str() => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (
                    vec![root],
                    vec![btreemap! {dir => Some(("replace_dir_with_file".to_string(), FileType::Regular))}],
                ),
            ],
        )
        .await?;

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_split_batch_in_linear_stacks_delete_file(fb: FacebookInit) -> Result<(), Error> {
        let repo = blobrepo_factory::TestRepoBuilder::new().build()?;
        let ctx = CoreContext::test_mock(fb);

        let file1 = "file1";
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file1, "content1")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .delete_file(file1)
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![root, child]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![
                (
                    vec![],
                    vec![btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (vec![root], vec![btreemap! {file1 => None}]),
            ],
        )
        .await?;

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_split_batch_in_linear_stacks_add_same_file(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = blobrepo_factory::TestRepoBuilder::new().build()?;
        let ctx = CoreContext::test_mock(fb);

        let file1 = "file1";
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file1, "content1")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file(file1, "content2")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(&ctx, &repo, vec![root, child]).await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![(
                vec![],
                vec![
                    btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))},
                    btreemap! {file1 => Some(("content2".to_string(), FileType::Regular))},
                ],
            )],
        )
        .await?;

        Ok(())
    }

    async fn assert_linear_stacks(
        ctx: &CoreContext,
        repo: &BlobRepo,
        actual: Vec<LinearStack>,
        expected: Vec<(
            Vec<ChangesetId>,
            Vec<BTreeMap<&str, Option<(String, FileType)>>>,
        )>,
    ) -> Result<(), Error> {
        let mut actual_res = vec![];
        for linear_stack in actual {
            let LinearStack {
                parents,
                file_changes,
            } = linear_stack;

            let mut paths_for_the_whole_stack = vec![];
            for (_, file_to_content) in file_changes {
                let mut paths = btreemap![];
                for (path, maybe_content) in file_to_content {
                    let maybe_content = match maybe_content {
                        Some((content_id, file_type)) => {
                            let content = filestore::fetch_concat(
                                &repo.get_blobstore(),
                                ctx.clone(),
                                filestore::FetchKey::Canonical(content_id),
                            )
                            .compat()
                            .await?;

                            let content = String::from_utf8(content.to_vec())?;
                            Some((content, file_type))
                        }
                        None => None,
                    };
                    paths.insert(path, maybe_content);
                }
                paths_for_the_whole_stack.push(paths);
            }

            actual_res.push((parents, paths_for_the_whole_stack));
        }

        let expected = expected
            .into_iter()
            .map(|(parents, linear_stack)| {
                let paths = linear_stack
                    .into_iter()
                    .map(|paths| {
                        let paths = paths
                            .into_iter()
                            .map(|(p, maybe_content)| (MPath::new(p).unwrap(), maybe_content))
                            .collect::<BTreeMap<_, _>>();
                        paths
                    })
                    .collect::<Vec<_>>();

                (parents, paths)
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_res, expected);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn batch_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let batch = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

            let cs_ids =
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                    .compat()
                    .try_collect::<Vec<_>>()
                    .await?;
            let fsnode_ids =
                derive_fsnode_in_batch(&ctx, &repo, cs_ids.clone().into_iter().rev().collect())
                    .await?;
            fsnode_ids.get(&master_cs_id).unwrap().clone()
        };

        let sequential = {
            let repo = linear::getrepo(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            RootFsnodeId::derive(ctx.clone(), repo.clone(), master_cs_id)
                .compat()
                .await?
                .fsnode_id()
                .clone()
        };

        assert_eq!(batch, sequential);
        Ok(())
    }
}
