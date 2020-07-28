/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use context::CoreContext;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future, TryStreamExt,
};
use manifest::ManifestOps;
use mononoke_types::{ChangesetId, FileChange, MPath};
use slog::info;
use std::collections::{BTreeMap, HashSet};

use crate::chunking::Chunker;
use crate::common::ChangesetArgs;
use crate::common::{
    create_and_save_bonsai, ChangesetArgsFactory, StackGroupChangesetArgsFactory, StackId,
    StackPosition,
};

const MAX_FILES_IN_DELETE_COMMIT: usize = 10000;

async fn get_all_working_copy_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<Vec<MPath>, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;

    let hg_cs = hg_cs_id.load(ctx.clone(), repo.blobstore()).await?;
    let paths_sorted = {
        let mut paths = hg_cs
            .manifestid()
            .list_leaf_entries(ctx.clone(), repo.blobstore().clone())
            .compat()
            .map_ok(|(mpath, _)| mpath)
            .try_collect::<Vec<MPath>>()
            .await?;

        paths.sort();
        paths
    };

    Ok(paths_sorted)
}

/// Create pre-merge delete commit stacks
/// Our gradual merge approach is like this:
/// ```text
///   M1
///   . \
///   . D11
///   .  |
///   . D12
///   .   |
///   M2   \
///   . \   |
///   . D21 |
///   .  |  |
///   . D22 |
///   .   | |
///   o    \|
///   |     |
///   o    PM
///
///   ^     ^
///   |      \
/// main DAG   merged repo's DAG
/// ```
/// Where:
/// - `M1`, `M2` - merge commits, each of which merges only a chunk
///   of the merged repo's DAG
/// - `PM` is a pre-merge master of the merged repo's DAG
/// - `D11`, `D12`, `D21` and `D22` are commits, which delete
///   a chunk of working copy each. Delete commmits are organized
///   into delete stacks, so that `D11` and `D12` progressively delete
///   more and more files. These commits are needed
///   to make partial merge possible. The union of stack tops working
///   copies (`D11` and `D21`) must equal the whole `PM` working copy. All
///   deletion stacks are parented by `PM`.
///
/// This function creates a set of such commits, parented
/// by `parent_bcs_id`. Files in the working copy are sharded
/// according to the `chunker` fn.
/// Return value is a list of commit stacks, starting from smaller
/// working copies.
pub async fn create_sharded_delete_commit_stacks<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    parent_bcs_id: ChangesetId,
    chunker: Chunker<MPath>,
    resulting_changeset_args: impl StackGroupChangesetArgsFactory,
) -> Result<Vec<Vec<ChangesetId>>, Error> {
    let all_mpaths_sorted: Vec<MPath> =
        get_all_working_copy_paths(ctx, repo, parent_bcs_id).await?;

    let chunked_mpaths = {
        let chunked_mpaths = chunker(all_mpaths_sorted.clone());
        // Sanity check: total number of files before and after chunking is the same
        // (together with a check below this also ensures that we didn't duplicate
        // any file)
        let before_count = all_mpaths_sorted.len();
        let after_count = chunked_mpaths
            .iter()
            .map(|chunk| chunk.len())
            .sum::<usize>();
        if before_count != after_count {
            return Err(anyhow!(
                "File counts before ({}) and after ({}) chunking are different",
                before_count,
                after_count,
            ));
        }

        // Sanity check that we have not dropped any file
        let before: HashSet<&MPath> = all_mpaths_sorted.iter().collect();
        let after: HashSet<&MPath> = chunked_mpaths
            .iter()
            .map(|chunk| chunk.iter())
            .flatten()
            .collect();
        if before != after {
            let lost_paths: Vec<&MPath> = before.difference(&after).take(5).map(|mp| *mp).collect();
            return Err(anyhow!(
                "Chunker lost some paths, for example: {:?}",
                lost_paths
            ));
        }

        chunked_mpaths
    };

    let delete_commit_creation_futs = chunked_mpaths.into_iter().enumerate().map(|(i, chunk)| {
        let changeset_args_factory = {
            // This is a bit ugly: explicit borrow is to avoid moving `resulting_changeset_args`
            // into a `move` closure. Yet closure needs to be `move`, so that `i` moves there
            let resulting_changeset_args = &resulting_changeset_args;
            move |stack_position: StackPosition| {
                resulting_changeset_args(StackId(i), stack_position)
            }
        };

        create_delete_commit_stack(
            ctx,
            repo,
            &parent_bcs_id,
            &all_mpaths_sorted,
            chunk.into_iter().collect(),
            changeset_args_factory,
            MAX_FILES_IN_DELETE_COMMIT,
        )
    });

    future::try_join_all(delete_commit_creation_futs).await
}

/// Given a list of files to keep in the working copy,
/// produce a stack of commits to gradually delete
/// all other files
async fn create_delete_commit_stack(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parent_bcs_id: &ChangesetId,
    all_files_sorted: &Vec<MPath>,
    files_to_keep: HashSet<MPath>,
    changeset_args_factory: impl ChangesetArgsFactory,
    max_files_in_delete_commit: usize,
) -> Result<Vec<ChangesetId>, Error> {
    let files_to_delete_sorted: Vec<&MPath> = {
        let mut files_to_delete = all_files_sorted
            .iter()
            .filter_map(|mpath| {
                if files_to_keep.contains(mpath) {
                    None
                } else {
                    Some(mpath)
                }
            })
            .collect::<Vec<_>>();
        files_to_delete.sort();
        files_to_delete
    };

    let mut parent = *parent_bcs_id;
    let mut stack = vec![];
    for (commit_id_in_stack, chunk) in files_to_delete_sorted
        .chunks(max_files_in_delete_commit)
        .enumerate()
    {
        let changeset_args = changeset_args_factory(StackPosition(commit_id_in_stack));
        let files_to_delete: Vec<MPath> = chunk.iter().map(|mpath| (**mpath).clone()).collect();
        info!(
            ctx.logger(),
            "Creating a delete commit for {} files, parented at {} with {:?}",
            files_to_delete.len(),
            parent,
            changeset_args
        );

        let next_commit =
            create_delete_commit(ctx, repo, parent, files_to_delete, changeset_args).await?;

        info!(
            ctx.logger(),
            "Created {} (child of {})", next_commit, parent
        );
        stack.push(next_commit);
        parent = next_commit;
    }

    info!(
        ctx.logger(),
        "Created deletion stack of length {}",
        stack.len()
    );

    Ok(stack)
}

async fn create_delete_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parent_bcs_id: ChangesetId,
    files_to_delete: Vec<MPath>,
    changeset_args: ChangesetArgs,
) -> Result<ChangesetId, Error> {
    let file_changes: BTreeMap<MPath, Option<FileChange>> = files_to_delete
        .into_iter()
        .map(|mpath| (mpath, None))
        .collect();

    create_and_save_bonsai(
        ctx,
        repo,
        vec![parent_bcs_id.clone()],
        file_changes,
        changeset_args,
    )
    .await
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures::future::try_join_all;
    use maplit::hashset;
    use mononoke_types::DateTime;
    use tests_utils::resolve_cs_id;

    #[fbinit::compat_test]
    async fn test_create_delete_commit_stack_single(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args = ChangesetArgs {
            author: "user".to_string(),
            message: "I like to delete it".to_string(),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let all_mpaths = get_all_working_copy_paths(&ctx, &repo, master_bcs_id).await?;
        let files_to_keep = hashset!(MPath::new("6")?);
        let deletion_stack = create_delete_commit_stack(
            &ctx,
            &repo,
            &master_bcs_id,
            &all_mpaths,
            files_to_keep.clone(),
            move |_| changeset_args.clone(),
            1_000_000, // max_files_in_delete_commit
        )
        .await?;

        assert_eq!(deletion_stack.len(), 1);

        let deletion_cs_id = deletion_stack.into_iter().next().unwrap();

        let new_all_mpaths: HashSet<_> = get_all_working_copy_paths(&ctx, &repo, deletion_cs_id)
            .await?
            .into_iter()
            .collect();
        assert_eq!(files_to_keep, new_all_mpaths);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commit_stack_multiple(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args = ChangesetArgs {
            author: "user".to_string(),
            message: "I like to delete it".to_string(),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let all_mpaths = get_all_working_copy_paths(&ctx, &repo, master_bcs_id).await?;
        let files_to_keep = hashset!(MPath::new("6")?);
        let deletion_stack_v1 = create_delete_commit_stack(
            &ctx,
            &repo,
            &master_bcs_id,
            &all_mpaths,
            files_to_keep.clone(),
            |_| changeset_args.clone(),
            1, // max_files_in_delete_commit
        )
        .await?;

        assert_eq!(deletion_stack_v1.len(), 10);
        let workin_copy_sizes: Vec<usize> =
            try_join_all(deletion_stack_v1.into_iter().map(|cs_id| {
                let ctx = &ctx;
                let repo = &repo;
                async move {
                    let wc_paths = get_all_working_copy_paths(ctx, repo, cs_id).await?;
                    Result::<_, Error>::Ok(wc_paths.len())
                }
            }))
            .await?;
        assert_eq!(workin_copy_sizes, vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);

        let deletion_stack_v2 = create_delete_commit_stack(
            &ctx,
            &repo,
            &master_bcs_id,
            &all_mpaths,
            files_to_keep.clone(),
            |_| changeset_args.clone(),
            3, // max_files_in_delete_commit
        )
        .await?;

        assert_eq!(deletion_stack_v2.len(), 4);
        let workin_copy_sizes: Vec<usize> =
            try_join_all(deletion_stack_v2.into_iter().map(|cs_id| {
                let ctx = &ctx;
                let repo = &repo;
                async move {
                    let wc_paths = get_all_working_copy_paths(ctx, repo, cs_id).await?;
                    Result::<_, Error>::Ok(wc_paths.len())
                }
            }))
            .await?;
        assert_eq!(workin_copy_sizes, vec![8, 5, 2, 1]);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commit_stacks_one_per_file(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args_factory = |num: StackId, _| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num.0),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            files
                .into_iter()
                .map(|file| vec![file])
                .collect::<Vec<Vec<MPath>>>()
        });

        let stacks: Vec<Vec<ChangesetId>> = create_sharded_delete_commit_stacks(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await?;

        let head_commits: Vec<ChangesetId> = stacks
            .into_iter()
            .map(|stack| stack.into_iter().last().unwrap())
            .collect();

        let all_mpaths_at_master: HashSet<_> =
            get_all_working_copy_paths(&ctx, &repo, master_bcs_id)
                .await?
                .into_iter()
                .collect();
        let all_mpaths_at_head_commits = future::try_join_all(
            head_commits
                .iter()
                .map(|cs_id| get_all_working_copy_paths(&ctx, &repo, *cs_id)),
        )
        .await?;

        for mpaths_at_commit in &all_mpaths_at_head_commits {
            assert_eq!(mpaths_at_commit.len(), 1);
        }

        let all_mpaths_at_head_commits: HashSet<MPath> = all_mpaths_at_head_commits
            .into_iter()
            .map(|mpaths_at_commit| mpaths_at_commit.into_iter())
            .flatten()
            .collect();
        assert_eq!(all_mpaths_at_head_commits, all_mpaths_at_master);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commit_stacks_two_stacks(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args_factory = |num: StackId, _| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num.0),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (v1, v2) = files.split_at(1);
            vec![v1.to_vec(), v2.to_vec()]
        });

        let commits: Vec<_> = create_sharded_delete_commit_stacks(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await?
        .into_iter()
        .map(|stack| stack.into_iter().last().unwrap())
        .collect();

        let all_mpaths_at_master: HashSet<_> =
            get_all_working_copy_paths(&ctx, &repo, master_bcs_id)
                .await?
                .into_iter()
                .collect();
        let all_mpaths_at_commits = future::try_join_all(
            commits
                .iter()
                .map(|cs_id| get_all_working_copy_paths(&ctx, &repo, *cs_id)),
        )
        .await?;

        assert_eq!(all_mpaths_at_commits[0].len(), 1);
        assert_eq!(
            all_mpaths_at_commits[1].len(),
            all_mpaths_at_master.len() - 1
        );

        let all_mpaths_at_commits: HashSet<MPath> = all_mpaths_at_commits
            .into_iter()
            .map(|mpaths_at_commit| mpaths_at_commit.into_iter())
            .flatten()
            .collect();
        assert_eq!(all_mpaths_at_commits, all_mpaths_at_master);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commit_stacks_invalid_chunker(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args_factory = |num: StackId, _| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num.0),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        // fewer files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![v2.to_vec()]
        });

        let stacks_res = create_sharded_delete_commit_stacks(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(stacks_res.is_err());

        // more files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![v2.to_vec(), v2.to_vec()]
        });

        let stacks_res = create_sharded_delete_commit_stacks(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(stacks_res.is_err());

        // correct number, but unrelated files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![vec![MPath::new("ababagalamaga").unwrap()], v2.to_vec()]
        });

        let stacks_res = create_sharded_delete_commit_stacks(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(stacks_res.is_err());

        // duplicated files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![v2.to_vec(), files]
        });

        let stacks_res = create_sharded_delete_commit_stacks(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(stacks_res.is_err());

        Ok(())
    }
}
