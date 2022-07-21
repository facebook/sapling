/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeMap;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::MPath;

pub type FileToContent = BTreeMap<MPath, Option<(ContentId, FileType)>>;
pub const DEFAULT_STACK_FILE_CHANGES_LIMIT: u64 = 10000;

#[derive(Copy, Clone, Debug)]
pub enum FileConflicts {
    /// Stacks should be split if files conflict in terms of change vs delete.
    ChangeDelete,

    /// Stacks should be split on any file change conflict.
    AnyChange,
}

pub struct SplitOptions {
    pub file_conflicts: FileConflicts,
    // Commits with copy info shouldn't be stacked with any other commit
    pub copy_info: bool,
    // Limits how many file changes should be in a single stack
    pub file_changes_limit: u64,
}

impl From<FileConflicts> for SplitOptions {
    fn from(fc: FileConflicts) -> Self {
        Self {
            file_conflicts: fc,
            copy_info: false,
            // Pick a reasonable default
            file_changes_limit: DEFAULT_STACK_FILE_CHANGES_LIMIT,
        }
    }
}

pub async fn split_batch_in_linear_stacks(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    batch: Vec<ChangesetId>,
    split_opts: SplitOptions,
) -> Result<Vec<LinearStack>, Error> {
    let bonsais = stream::iter(
        batch
            .into_iter()
            .map(|bcs_id| async move { bcs_id.load(ctx, blobstore).await }),
    )
    .buffered(100)
    .try_collect::<Vec<_>>()
    .await?;
    split_bonsais_in_linear_stacks(&bonsais, split_opts)
}

/// We follow a few rules when splitting a batch in the stacks:
/// 1) Merges go to a separate batch
/// 2) If two commits have two files where one is a prefix of another, then they
///    go to a separate stacks (because of the way bonsai interprets these files)
/// 3) If there are file conflicts (see SplitOptions for details) then a commit go to a
///    separate stack.
/// 4) We check any other requirements set by the SplitOptions.
pub fn split_bonsais_in_linear_stacks(
    bonsais: &[BonsaiChangeset],
    split_opts: SplitOptions,
) -> Result<Vec<LinearStack>, Error> {
    let start_bcs = match bonsais.first() {
        Some(val) => val,
        None => {
            return Ok(vec![]);
        }
    };

    let mut linear_stacks = Vec::new();
    let mut cur_linear_stack = LinearStack::new(start_bcs.parents().collect::<Vec<_>>());
    cur_linear_stack.push(start_bcs);

    for (prev_bcs, bcs) in bonsais.iter().tuple_windows() {
        if !cur_linear_stack.can_be_in_same_linear_stack(prev_bcs, bcs, &split_opts) {
            linear_stacks.push(cur_linear_stack);
            cur_linear_stack = LinearStack::new(bcs.parents().collect::<Vec<_>>());
        }

        cur_linear_stack.push(bcs);
    }

    linear_stacks.push(cur_linear_stack);

    Ok(linear_stacks)
}

// Stores a linear stack.
#[derive(Clone, Debug)]
pub struct LinearStack {
    pub parents: Vec<ChangesetId>,
    pub stack_items: Vec<StackItem>,
    total_file_changes_len: u64,
}

#[derive(Clone, Debug)]
pub struct StackItem {
    pub cs_id: ChangesetId,
    pub combined_file_changes: FileToContent,
    pub per_commit_file_changes: FileToContent,
}

impl LinearStack {
    fn new(parents: Vec<ChangesetId>) -> Self {
        Self {
            parents,
            stack_items: vec![],
            total_file_changes_len: 0,
        }
    }

    fn push(&mut self, bcs: &BonsaiChangeset) {
        let cs_id = bcs.get_changeset_id();
        let file_changes = bcs
            .file_changes()
            .map(|(path, fc)| {
                (
                    path.clone(),
                    fc.simplify().map(|bc| (bc.content_id(), bc.file_type())),
                )
            })
            .collect::<BTreeMap<_, _>>();

        self.total_file_changes_len += file_changes.len() as u64;
        let mut combined_file_changes = self.get_last_file_changes().cloned().unwrap_or_default();
        combined_file_changes.extend(file_changes.clone());
        self.stack_items.push(StackItem {
            cs_id,
            combined_file_changes,
            per_commit_file_changes: file_changes,
        });
    }

    fn can_be_in_same_linear_stack(
        &self,
        prev: &BonsaiChangeset,
        next: &BonsaiChangeset,
        split_opts: &SplitOptions,
    ) -> bool {
        // Each merge should go in a separate stack
        if prev.is_merge() || next.is_merge() {
            return false;
        }

        if split_opts.copy_info {
            if has_copy_info(prev) || has_copy_info(next) {
                return false;
            }
        }

        // The next commit should be stacked on top of the previous one
        let prev_cs_id = prev.get_changeset_id();
        if !next.parents().any(|p| p == prev_cs_id) {
            return false;
        }

        // Check if stack will get too big
        let next_len = next.file_changes().len() as u64;
        if self.total_file_changes_len + next_len > split_opts.file_changes_limit {
            return false;
        }

        // There must be no file conflicts when adding the new changes.
        if let Some(cur_file_changes) = self.get_last_file_changes() {
            let new_file_changes = next.file_changes().collect();
            if has_file_conflict(
                cur_file_changes,
                new_file_changes,
                split_opts.file_conflicts,
            ) {
                return false;
            }
        }

        true
    }

    fn get_last_file_changes(&self) -> Option<&FileToContent> {
        self.stack_items
            .last()
            .map(|item| &item.combined_file_changes)
    }
}

fn has_copy_info(cs: &BonsaiChangeset) -> bool {
    for (_, fc) in cs.file_changes() {
        use FileChange::*;
        match fc {
            Change(fc) => {
                if fc.copy_from().is_some() {
                    return true;
                }
            }
            UntrackedChange(_) | Deletion | UntrackedDeletion => {}
        }
    }

    false
}

/// Returns true if:
/// 1) any of the files in `left` are prefix of any file in `right` and vice-versa
/// 2) File with the same name was deleted in `left` and added in `right` or vice-versa
fn has_file_conflict(
    left: &FileToContent,
    right: BTreeMap<&MPath, &FileChange>,
    file_conflicts: FileConflicts,
) -> bool {
    let mut left = left.iter().peekable();
    let mut right = right.into_iter().peekable();
    let mut state = (left.next(), right.next());
    loop {
        state = match state {
            (Some((l_path, l_content)), Some((r_path, r_file_change))) => {
                match l_path.cmp(r_path) {
                    CmpOrdering::Equal => {
                        match file_conflicts {
                            FileConflicts::ChangeDelete => {
                                if l_content.is_some() != r_file_change.is_changed() {
                                    // File is deleted and modified in two
                                    // different commits - this is a conflict,
                                    // they can't be in the same stack
                                    return true;
                                }
                            }
                            FileConflicts::AnyChange => {
                                return true;
                            }
                        }

                        // It's possible for a single conflict to have a path
                        // conflict (usually it happens when a file is replaced
                        // with a directory). The code below checks if we have
                        // a conflict like that and exists early if we do.
                        if let Some((next_l_path, _)) = left.peek() {
                            if r_path.is_prefix_of(*next_l_path) {
                                return true;
                            }
                        } else if let Some((next_r_path, _)) = right.peek() {
                            if l_path.is_prefix_of(*next_r_path) {
                                return true;
                            }
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
                }
            }
            _ => break,
        };
    }

    false
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::BlobRepo;
    use fbinit::FacebookInit;
    use maplit::btreemap;
    use mononoke_types::FileType;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![second],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, second],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks.clone(),
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

        // Check that per_commit_file_changes are correct
        assert_eq!(
            linear_stacks[0].stack_items[0]
                .per_commit_file_changes
                .keys()
                .collect::<Vec<_>>(),
            vec![&MPath::new(file1)?]
        );

        assert_eq!(
            linear_stacks[0].stack_items[1]
                .per_commit_file_changes
                .keys()
                .collect::<Vec<_>>(),
            vec![&MPath::new(file2)?]
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_with_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, second],
            SplitOptions {
                file_conflicts: FileConflicts::ChangeDelete,
                copy_info: false,
                file_changes_limit: 1,
            },
        )
        .await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks.clone(),
            vec![
                (
                    vec![],
                    vec![btreemap! {file1 => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (
                    vec![root],
                    vec![btreemap! {
                        file2 => Some(("content2".to_string(), FileType::Regular)),
                    }],
                ),
            ],
        )
        .await?;

        // Check that per_commit_file_changes are correct
        assert_eq!(
            linear_stacks[0].stack_items[0]
                .per_commit_file_changes
                .keys()
                .collect::<Vec<_>>(),
            vec![&MPath::new(file1)?]
        );

        assert_eq!(
            linear_stacks[1].stack_items[0]
                .per_commit_file_changes
                .keys()
                .collect::<Vec<_>>(),
            vec![&MPath::new(file2)?]
        );

        Ok(())
    }
    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_merge(fb: FacebookInit) -> Result<(), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![p1, merge],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![p1, p2],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_replace_dir_with_file(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        let ctx = CoreContext::test_mock(fb);

        let dir = "dir";
        let file1 = format!("{}/file1", dir);
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file1.as_str(), "content1")
            .commit()
            .await?;
        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file(dir, "replace_dir_with_file")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, child],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_delete_file(fb: FacebookInit) -> Result<(), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, child],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_add_same_file(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
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

        // With ChangeDelete, the stack is combined.
        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, child],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
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

        // With AnyChange, the stack is split.
        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, child],
            FileConflicts::AnyChange.into(),
        )
        .await?;
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
                    vec![root],
                    vec![btreemap! {file1 => Some(("content2".to_string(), FileType::Regular))}],
                ),
            ],
        )
        .await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_replace_file_with_dir(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir", "content1")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![root])
            .delete_file("dir")
            .add_file("dir/file", "content1")
            .commit()
            .await?;
        let third = CreateCommitContext::new(&ctx, &repo, vec![second])
            .delete_file("dir")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, second, third],
            FileConflicts::ChangeDelete.into(),
        )
        .await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![
                (
                    vec![],
                    vec![btreemap! {"dir" => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (
                    vec![root],
                    vec![btreemap! {
                        "dir" => None,
                        "dir/file" => Some(("content1".to_string(), FileType::Regular))
                    }],
                ),
                (
                    vec![second],
                    vec![btreemap! {
                        "dir" => None,
                    }],
                ),
            ],
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_batch_in_linear_stacks_copy_info(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir", "content1")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file_with_copy_info("copiedfile", "content1", (root, "dir"))
            .commit()
            .await?;
        let third = CreateCommitContext::new(&ctx, &repo, vec![second])
            .add_file("dir2", "content2")
            .commit()
            .await?;

        let linear_stacks = split_batch_in_linear_stacks(
            &ctx,
            repo.blobstore(),
            vec![root, second, third],
            SplitOptions {
                file_conflicts: FileConflicts::ChangeDelete,
                copy_info: true,
                file_changes_limit: DEFAULT_STACK_FILE_CHANGES_LIMIT,
            },
        )
        .await?;
        assert_linear_stacks(
            &ctx,
            &repo,
            linear_stacks,
            vec![
                (
                    vec![],
                    vec![btreemap! {"dir" => Some(("content1".to_string(), FileType::Regular))}],
                ),
                (
                    vec![root],
                    vec![btreemap! {
                        "copiedfile" => Some(("content1".to_string(), FileType::Regular))
                    }],
                ),
                (
                    vec![second],
                    vec![btreemap! {
                        "dir2" => Some(("content2".to_string(), FileType::Regular))
                    }],
                ),
            ],
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
                stack_items,
                ..
            } = linear_stack;

            let mut paths_for_the_whole_stack = vec![];
            for item in stack_items {
                let file_to_content = item.combined_file_changes;
                let mut paths = btreemap![];
                for (path, maybe_content) in file_to_content {
                    let maybe_content = match maybe_content {
                        Some((content_id, file_type)) => {
                            let content = filestore::fetch_concat(
                                &repo.get_blobstore(),
                                ctx,
                                filestore::FetchKey::Canonical(content_id),
                            )
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
                        paths
                            .into_iter()
                            .map(|(p, maybe_content)| (MPath::new(p).unwrap(), maybe_content))
                            .collect::<BTreeMap<_, _>>()
                    })
                    .collect::<Vec<_>>();

                (parents, paths)
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_res, expected);

        Ok(())
    }
}
