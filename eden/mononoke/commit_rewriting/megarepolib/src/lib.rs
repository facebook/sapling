/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
#![deny(warnings)]

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use context::CoreContext;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future, Stream, TryStreamExt,
};
use itertools::Itertools;
use manifest::ManifestOps;
use mercurial_types::{
    blobs::{HgBlobChangeset, HgBlobEnvelope},
    HgChangesetId, MPath,
};
use mononoke_types::{ChangesetId, ContentId, FileChange, FileType};
use movers::Mover;
use slog::info;
use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroU64;

pub mod chunking;
pub mod common;
use crate::chunking::Chunker;
use crate::common::{
    create_and_save_bonsai, create_save_and_generate_hg_changeset, ChangesetArgs,
    ChangesetArgsFactory,
};

const BUFFER_SIZE: usize = 100;
const REPORTING_INTERVAL_FILES: usize = 10000;

struct FileMove {
    old_path: MPath,
    maybe_new_path: Option<MPath>,
    file_type: FileType,
    file_size: u64,
    content_id: ContentId,
}

fn get_all_file_moves<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    hg_cs: HgBlobChangeset,
    path_converter: &'a Mover,
) -> impl Stream<Item = Result<FileMove, Error>> + 'a {
    hg_cs
        .manifestid()
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .compat()
        .try_filter_map(move |(old_path, (file_type, filenode_id))| async move {
            let maybe_new_path = path_converter(&old_path).unwrap();
            if Some(&old_path) == maybe_new_path.as_ref() {
                // path does not need to be changed, drop from the stream
                Ok(None)
            } else {
                // path needs to be changed (or deleted), keep in the stream
                Ok(Some((old_path, maybe_new_path, file_type, filenode_id)))
            }
        })
        .map_ok({
            move |(old_path, maybe_new_path, file_type, filenode_id)| {
                async move {
                    let file_envelope = filenode_id.load(ctx.clone(), repo.blobstore()).await?;

                    // Note: it is always safe to unwrap here, since
                    // `HgFileEnvelope::get_size()` always returns `Some()`
                    // The return type is `Option` to acommodate `HgManifestEnvelope`
                    // which returns `None`.
                    let file_size = file_envelope.get_size().unwrap();

                    let file_move = FileMove {
                        old_path,
                        maybe_new_path,
                        file_type,
                        file_size,
                        content_id: file_envelope.content_id(),
                    };
                    Ok(file_move)
                }
            }
        })
        .try_buffer_unordered(BUFFER_SIZE)
}

pub async fn perform_move<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    parent_bcs_id: ChangesetId,
    path_converter: Mover,
    resulting_changeset_args: ChangesetArgs,
) -> Result<HgChangesetId, Error> {
    let mut stack = perform_stack_move_impl(
        ctx,
        repo,
        parent_bcs_id,
        path_converter,
        |file_moves| vec![file_moves],
        move |_| resulting_changeset_args.clone(),
    )
    .await?;

    if stack.len() == 1 {
        stack
            .pop()
            .ok_or_else(|| anyhow!("not a single commit was created"))
    } else {
        Err(anyhow!(
            "wrong number of commits was created. Expected 1, created {}: {:?}",
            stack.len(),
            stack
        ))
    }
}

/// Move files according to path_converter in a stack of commits.
/// Each commit won't have more than `max_num_of_moves_in_commit` files.
/// Creating a stack of commits might be desirable if we want to keep each commit smaller.
pub async fn perform_stack_move<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    parent_bcs_id: ChangesetId,
    path_converter: Mover,
    max_num_of_moves_in_commit: NonZeroU64,
    resulting_changeset_args: impl ChangesetArgsFactory,
) -> Result<Vec<HgChangesetId>, Error> {
    perform_stack_move_impl(
        ctx,
        repo,
        parent_bcs_id,
        path_converter,
        |file_changes| {
            file_changes
                .into_iter()
                .chunks(max_num_of_moves_in_commit.get() as usize)
                .into_iter()
                .map(|chunk| chunk.collect())
                .collect()
        },
        resulting_changeset_args,
    )
    .await
}

async fn perform_stack_move_impl<'a, Chunker>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    mut parent_bcs_id: ChangesetId,
    path_converter: Mover,
    chunker: Chunker,
    resulting_changeset_args: impl ChangesetArgsFactory,
) -> Result<Vec<HgChangesetId>, Error>
where
    Chunker: Fn(Vec<FileMove>) -> Vec<Vec<FileMove>>,
{
    let parent_hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), parent_bcs_id)
        .compat()
        .await?;

    let parent_hg_cs = parent_hg_cs_id.load(ctx.clone(), repo.blobstore()).await?;

    let mut file_changes = get_all_file_moves(&ctx, &repo, parent_hg_cs, &path_converter)
        .try_fold(vec![], {
            move |mut collected, file_move| {
                collected.push(file_move);
                if collected.len() % REPORTING_INTERVAL_FILES == 0 {
                    info!(ctx.logger(), "Processed {} files", collected.len());
                }
                future::ready(Ok(collected))
            }
        })
        .await?;

    let mut res = vec![];
    file_changes.sort_unstable_by(|first, second| first.old_path.cmp(&second.old_path));
    let chunks_iter = chunker(file_changes);

    for (idx, chunk) in chunks_iter.into_iter().enumerate() {
        let mut file_changes = BTreeMap::new();
        for file_move in chunk {
            file_changes.insert(file_move.old_path.clone(), None);
            if let Some(to) = file_move.maybe_new_path {
                let fc = FileChange::new(
                    file_move.content_id,
                    file_move.file_type,
                    file_move.file_size,
                    Some((file_move.old_path, parent_bcs_id)),
                );
                file_changes.insert(to, Some(fc));
            }
        }

        let hg_cs_id = create_save_and_generate_hg_changeset(
            &ctx,
            &repo,
            vec![parent_bcs_id],
            file_changes,
            resulting_changeset_args(idx),
        )
        .await?;

        parent_bcs_id = repo
            .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
            .compat()
            .await?
            .ok_or(anyhow!("not found bonsai commit for {}", hg_cs_id))?;
        res.push(hg_cs_id)
    }

    Ok(res)
}

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

/// Create pre-merge delete commits
/// Our gradual merge approach is like this:
/// ```text
///   M1
///   . \
///   .  D1
///   M2   \
///   . \   |
///   .  D2 |
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
/// - `D1` and `D2` are commits, which delete everything except
///   for a chunk of working copy each. These commits are needed
///   to make partial merge possible. The union of `D1`, `D2`, ...
///   working copies must equal the whole `PM` working copy. These
///   deletion commits do not form a stack, they are all parented
///   by `PM`
///
/// This function creates a set of such commits, parented
/// by `parent_bcs_id`. Files in the working copy are sharded
/// according to the `chunker` fn.
pub async fn create_sharded_delete_commits<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    parent_bcs_id: ChangesetId,
    chunker: Chunker<MPath>,
    resulting_changeset_args: impl ChangesetArgsFactory,
) -> Result<Vec<ChangesetId>, Error> {
    let all_mpaths: Vec<MPath> = get_all_working_copy_paths(ctx, repo, parent_bcs_id).await?;

    let chunked_mpaths = {
        let chunked_mpaths = chunker(all_mpaths.clone());
        // Sanity check: total number of files before and after chunking is the same
        // (together with a check below this also ensures that we didn't duplicate
        // any file)
        let before_count = all_mpaths.len();
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
        let before: HashSet<&MPath> = all_mpaths.iter().collect();
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
        let changeset_args = resulting_changeset_args(i);
        create_delete_commit(
            ctx,
            repo,
            &parent_bcs_id,
            &all_mpaths,
            chunk.into_iter().collect(),
            changeset_args,
        )
    });

    future::try_join_all(delete_commit_creation_futs).await
}

async fn create_delete_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parent_bcs_id: &ChangesetId,
    all_files: &Vec<MPath>,
    files_to_keep: HashSet<MPath>,
    changeset_args: ChangesetArgs,
) -> Result<ChangesetId, Error> {
    let file_changes: BTreeMap<MPath, Option<FileChange>> = all_files
        .iter()
        .filter_map(|mpath| {
            if files_to_keep.contains(mpath) {
                None
            } else {
                Some((mpath.clone(), None))
            }
        })
        .collect();

    info!(
        ctx.logger(),
        "Creating a delete commit for {} files with {:?}",
        file_changes.len(),
        changeset_args
    );

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
    use blobrepo_hg::BlobRepoHg;
    use cloned::cloned;
    use fbinit::FacebookInit;
    use fixtures::{linear, many_files_dirs};
    use futures::{compat::Future01CompatExt, future::TryFutureExt};
    use futures_old::{stream::Stream, Future};
    use maplit::{btreemap, hashset};
    use mercurial_types::HgChangesetId;
    use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, DateTime};
    use std::str::FromStr;
    use std::sync::Arc;
    use tests_utils::resolve_cs_id;

    fn identity_mover(p: &MPath) -> Result<Option<MPath>> {
        Ok(Some(p.clone()))
    }

    fn skip_one(p: &MPath) -> Result<Option<MPath>> {
        if &MPath::new("dir1/file_1_in_dir1").unwrap() == p {
            return Ok(None);
        }

        Ok(Some(p.clone()))
    }

    fn shift_one(p: &MPath) -> Result<Option<MPath>> {
        if &MPath::new("dir1/file_1_in_dir1").unwrap() == p {
            return Ok(Some(MPath::new("newdir/dir1/file_1_in_dir1").unwrap()));
        }

        Ok(Some(p.clone()))
    }

    fn shift_one_skip_another(p: &MPath) -> Result<Option<MPath>> {
        if &MPath::new("dir1/file_1_in_dir1").unwrap() == p {
            return Ok(Some(MPath::new("newdir/dir1/file_1_in_dir1").unwrap()));
        }

        if &MPath::new("dir2/file_1_in_dir2").unwrap() == p {
            return Ok(None);
        }

        Ok(Some(p.clone()))
    }

    fn shift_all(p: &MPath) -> Result<Option<MPath>> {
        Ok(Some(MPath::new("moved_dir")?.join(p)))
    }

    async fn prepare(
        fb: FacebookInit,
    ) -> (
        CoreContext,
        BlobRepo,
        HgChangesetId,
        ChangesetId,
        ChangesetArgs,
    ) {
        let repo = many_files_dirs::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let hg_cs_id = HgChangesetId::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();
        let bcs_id: ChangesetId = repo
            .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
            .compat()
            .await
            .unwrap()
            .unwrap();
        let changeset_args = ChangesetArgs {
            author: "user".to_string(),
            message: "I like to move it".to_string(),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };
        (ctx, repo, hg_cs_id, bcs_id, changeset_args)
    }

    async fn get_bonsai_by_hg_cs_id(
        ctx: CoreContext,
        repo: BlobRepo,
        hg_cs_id: HgChangesetId,
    ) -> BonsaiChangeset {
        let bcs_id = repo
            .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
            .compat()
            .await
            .unwrap()
            .unwrap();
        bcs_id.load(ctx.clone(), repo.blobstore()).await.unwrap()
    }

    #[fbinit::test]
    fn test_do_not_move_anything(fb: FacebookInit) {
        async_unit::tokio_unit_test(async move {
            let (ctx, repo, _hg_cs_id, bcs_id, changeset_args) = prepare(fb).await;
            let newcs = perform_move(
                &ctx,
                &repo,
                bcs_id,
                Arc::new(identity_mover),
                changeset_args,
            )
            .await
            .unwrap();
            let newcs = get_bonsai_by_hg_cs_id(ctx.clone(), repo.clone(), newcs).await;

            let BonsaiChangesetMut {
                parents,
                author: _,
                author_date: _,
                committer: _,
                committer_date: _,
                message: _,
                extra: _,
                file_changes,
            } = newcs.into_mut();
            assert_eq!(parents, vec![bcs_id]);
            assert_eq!(file_changes, btreemap! {});
        });
    }

    #[fbinit::test]
    fn test_drop_file(fb: FacebookInit) {
        async_unit::tokio_unit_test(async move {
            let (ctx, repo, _hg_cs_id, bcs_id, changeset_args) = prepare(fb).await;
            let newcs = perform_move(&ctx, &repo, bcs_id, Arc::new(skip_one), changeset_args)
                .await
                .unwrap();
            let newcs = get_bonsai_by_hg_cs_id(ctx.clone(), repo.clone(), newcs).await;

            let BonsaiChangesetMut {
                parents,
                author: _,
                author_date: _,
                committer: _,
                committer_date: _,
                message: _,
                extra: _,
                file_changes,
            } = newcs.into_mut();
            assert_eq!(parents, vec![bcs_id]);
            assert_eq!(
                file_changes,
                btreemap! {
                    MPath::new("dir1/file_1_in_dir1").unwrap() => None
                }
            );
        });
    }

    #[fbinit::test]
    fn test_shift_path_by_one(fb: FacebookInit) {
        async_unit::tokio_unit_test(async move {
            let (ctx, repo, _hg_cs_id, bcs_id, changeset_args) = prepare(fb).await;
            let newcs = perform_move(&ctx, &repo, bcs_id, Arc::new(shift_one), changeset_args)
                .await
                .unwrap();
            let newcs = get_bonsai_by_hg_cs_id(ctx.clone(), repo.clone(), newcs).await;

            let BonsaiChangesetMut {
                parents,
                author: _,
                author_date: _,
                committer: _,
                committer_date: _,
                message: _,
                extra: _,
                file_changes,
            } = newcs.into_mut();
            assert_eq!(parents, vec![bcs_id]);
            let old_path = MPath::new("dir1/file_1_in_dir1").unwrap();
            let new_path = MPath::new("newdir/dir1/file_1_in_dir1").unwrap();
            assert_eq!(file_changes[&old_path], None);
            let file_change = file_changes[&new_path].as_ref().unwrap();
            assert_eq!(file_change.copy_from(), Some((old_path, bcs_id)).as_ref());
        });
    }

    async fn get_working_copy_contents(
        ctx: CoreContext,
        repo: BlobRepo,
        hg_cs_id: HgChangesetId,
    ) -> BTreeMap<MPath, (FileType, ContentId)> {
        let hg_cs = hg_cs_id.load(ctx.clone(), repo.blobstore()).await.unwrap();
        hg_cs
            .manifestid()
            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
            .and_then({
                cloned!(ctx, repo);
                move |(path, (file_type, filenode_id))| {
                    filenode_id
                        .load(ctx.clone(), repo.blobstore())
                        .compat()
                        .from_err()
                        .map(move |env| (path, (file_type, env.content_id())))
                }
            })
            .collect()
            .compat()
            .await
            .unwrap()
            .into_iter()
            .collect()
    }

    #[fbinit::test]
    fn test_performed_move(fb: FacebookInit) {
        async_unit::tokio_unit_test(async move {
            let (ctx, repo, old_hg_cs_id, old_bcs_id, changeset_args) = prepare(fb).await;
            let new_hg_cs_id = perform_move(
                &ctx,
                &repo,
                old_bcs_id,
                Arc::new(shift_one_skip_another),
                changeset_args,
            )
            .await
            .unwrap();
            let mut old_wc =
                get_working_copy_contents(ctx.clone(), repo.clone(), old_hg_cs_id).await;
            let mut new_wc =
                get_working_copy_contents(ctx.clone(), repo.clone(), new_hg_cs_id).await;
            let _removed_file = old_wc.remove(&MPath::new("dir2/file_1_in_dir2").unwrap());
            let old_moved_file = old_wc.remove(&MPath::new("dir1/file_1_in_dir1").unwrap());
            let new_moved_file = new_wc.remove(&MPath::new("newdir/dir1/file_1_in_dir1").unwrap());
            // Same file should live in both locations
            assert_eq!(old_moved_file, new_moved_file);
            // After removing renamed and removed files, both working copies should be identical
            assert_eq!(old_wc, new_wc);
        });
    }

    #[fbinit::compat_test]
    async fn test_stack_move(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let old_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let create_cs_args = |num| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let stack = perform_stack_move(
            &ctx,
            &repo,
            old_bcs_id,
            Arc::new(shift_all),
            NonZeroU64::new(1).unwrap(),
            create_cs_args,
        )
        .await?;

        // 11 files, so create 1 commit for each
        assert_eq!(stack.len(), 11);

        let stack = perform_stack_move(
            &ctx,
            &repo,
            old_bcs_id,
            Arc::new(shift_all),
            NonZeroU64::new(2).unwrap(),
            create_cs_args,
        )
        .await?;
        assert_eq!(stack.len(), 6);

        let last_hg_cs_id = stack.last().unwrap();
        let last_hg_cs = last_hg_cs_id.load(ctx.clone(), repo.blobstore()).await?;

        let leaf_entries = last_hg_cs
            .manifestid()
            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
            .compat()
            .try_collect::<Vec<_>>()
            .await?;

        assert_eq!(leaf_entries.len(), 11);
        let prefix = MPath::new("moved_dir")?;
        for (leaf, _) in &leaf_entries {
            assert!(prefix.is_prefix_of(leaf));
        }

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commit(fb: FacebookInit) -> Result<(), Error> {
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
        let deletion_cs_id = create_delete_commit(
            &ctx,
            &repo,
            &master_bcs_id,
            &all_mpaths,
            files_to_keep.clone(),
            changeset_args,
        )
        .await?;
        let new_all_mpaths: HashSet<_> = get_all_working_copy_paths(&ctx, &repo, deletion_cs_id)
            .await?
            .into_iter()
            .collect();
        assert_eq!(files_to_keep, new_all_mpaths);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commits_one_per_file(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args_factory = |num| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num),
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
        let commits = create_sharded_delete_commits(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await?;

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

        for mpaths_at_commit in &all_mpaths_at_commits {
            assert_eq!(mpaths_at_commit.len(), 1);
        }

        let all_mpaths_at_commits: HashSet<MPath> = all_mpaths_at_commits
            .into_iter()
            .map(|mpaths_at_commit| mpaths_at_commit.into_iter())
            .flatten()
            .collect();
        assert_eq!(all_mpaths_at_commits, all_mpaths_at_master);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_create_delete_commits_two_commits(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args_factory = |num| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (v1, v2) = files.split_at(1);
            vec![v1.to_vec(), v2.to_vec()]
        });

        let commits = create_sharded_delete_commits(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await?;

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
    async fn test_create_delete_commits_invalid_chunker(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;

        let changeset_args_factory = |num| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        // fewer files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![v2.to_vec()]
        });

        let commits_res = create_sharded_delete_commits(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(commits_res.is_err());

        // more files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![v2.to_vec(), v2.to_vec()]
        });

        let commits_res = create_sharded_delete_commits(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(commits_res.is_err());

        // correct number, but unrelated files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![vec![MPath::new("ababagalamaga").unwrap()], v2.to_vec()]
        });

        let commits_res = create_sharded_delete_commits(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(commits_res.is_err());

        // duplicated files
        let chunker: Chunker<MPath> = Box::new(|files: Vec<MPath>| {
            let (_, v2) = files.split_at(1);
            vec![v2.to_vec(), files]
        });

        let commits_res = create_sharded_delete_commits(
            &ctx,
            &repo,
            master_bcs_id,
            chunker,
            changeset_args_factory,
        )
        .await;

        assert!(commits_res.is_err());

        Ok(())
    }
}
