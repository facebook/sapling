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
use std::collections::BTreeMap;
use std::num::NonZeroU64;

pub mod chunking;
pub mod common;
pub mod pre_merge_deletes;

use crate::common::{
    create_save_and_generate_hg_changeset, ChangesetArgs, ChangesetArgsFactory, StackPosition,
};
pub use pre_merge_deletes::create_sharded_delete_commit_stacks;

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
            resulting_changeset_args(StackPosition(idx)),
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
    use maplit::btreemap;
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
        let create_cs_args = |num: StackPosition| ChangesetArgs {
            author: "user".to_string(),
            message: format!("I like to delete it: {}", num.0),
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
}
