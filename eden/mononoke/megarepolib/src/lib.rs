// (c) Facebook, Inc. and its affiliates. Confidential and proprietary.

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures_old::{
    future::{ok, Future, FutureResult},
    stream::{iter_ok, Stream},
};
use manifest::ManifestOps;
use mercurial_types::{
    blobs::{HgBlobChangeset, HgBlobEnvelope},
    HgChangesetId, MPath,
};
use mononoke_types::{ChangesetId, ContentId, FileChange, FileType};
use movers::Mover;
use slog::info;
use std::collections::BTreeMap;
use std::iter::Iterator;

pub mod common;
use crate::common::{create_and_save_changeset, ChangesetArgs};

const BUFFER_SIZE: usize = 100;
const REPORTING_INTERVAL_FILES: usize = 10000;

fn get_file_changes(
    old_path: MPath,
    maybe_new_path: Option<MPath>,
    file_type: FileType,
    file_size: u64,
    content_id: ContentId,
    parent_cs: ChangesetId,
) -> Vec<(MPath, Option<FileChange>)> {
    // Remove old file
    let mut res = vec![(old_path.clone(), None)];
    if let Some(new_path) = maybe_new_path {
        // We are not just dropping the file,
        // so let's add it's new location
        let file_change = FileChange::new(
            content_id,
            file_type,
            file_size,
            Some((old_path, parent_cs)),
        );
        res.push((new_path, Some(file_change)))
    }
    res
}

fn get_move_file_changes(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs: HgBlobChangeset,
    parent_cs_id: ChangesetId,
    path_converter: Mover,
) -> impl Stream<Item = (MPath, Option<FileChange>), Error = Error> {
    hg_cs
        .manifestid()
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .filter_map(move |(old_path, (file_type, filenode_id))| {
            let maybe_new_path = path_converter(&old_path).unwrap();
            if Some(old_path.clone()) == maybe_new_path {
                // path does not need to be changed, drop from the stream
                None
            } else {
                // path needs to be changed (or deleted), keep in the stream
                Some((old_path.clone(), maybe_new_path, file_type, filenode_id))
            }
        })
        .map({
            cloned!(ctx, repo, parent_cs_id);
            move |(old_path, maybe_new_path, file_type, filenode_id)| {
                filenode_id
                    .load(ctx.clone(), repo.blobstore())
                    .from_err()
                    .map(move |file_envelope| {
                        // Note: it is always safe to unwrap here, since
                        // `HgFileEnvelope::get_size()` always returns `Some()`
                        // The return type is `Option` to acommodate `HgManifestEnvelope`
                        // which returns `None`.
                        let file_size = file_envelope.get_size().unwrap();
                        iter_ok(get_file_changes(
                            old_path,
                            maybe_new_path,
                            file_type,
                            file_size,
                            file_envelope.content_id(),
                            parent_cs_id,
                        ))
                    })
            }
        })
        .buffer_unordered(BUFFER_SIZE)
        .flatten()
}

pub fn perform_move(
    ctx: CoreContext,
    repo: BlobRepo,
    parent_bcs_id: ChangesetId,
    path_converter: Mover,
    resulting_changeset_args: ChangesetArgs,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    repo.clone()
        .get_hg_from_bonsai_changeset(ctx.clone(), parent_bcs_id)
        .and_then({
            cloned!(repo, parent_bcs_id);
            move |parent_hg_cs_id| {
                parent_hg_cs_id
                    .load(ctx.clone(), repo.blobstore())
                    .from_err()
                    .and_then({
                        cloned!(ctx, parent_bcs_id, repo);
                        move |parent_hg_cs| {
                            get_move_file_changes(
                                ctx.clone(),
                                repo.clone(),
                                parent_hg_cs,
                                parent_bcs_id.clone(),
                                path_converter,
                            )
                            .fold(vec![], {
                                cloned!(ctx);
                                move |mut collected, item| {
                                    collected.push(item);
                                    if collected.len() % REPORTING_INTERVAL_FILES == 0 {
                                        info!(ctx.logger(), "Processed {} files", collected.len());
                                    }
                                    let res: FutureResult<Vec<_>, Error> = ok(collected);
                                    res
                                }
                            })
                            .and_then({
                                cloned!(ctx, repo, parent_bcs_id);
                                move |file_changes: Vec<(MPath, Option<FileChange>)>| {
                                    let file_changes: BTreeMap<_, _> =
                                        file_changes.into_iter().collect();
                                    create_and_save_changeset(
                                        ctx,
                                        repo,
                                        vec![parent_bcs_id],
                                        file_changes,
                                        resulting_changeset_args,
                                    )
                                }
                            })
                        }
                    })
            }
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use fbinit::FacebookInit;
    use fixtures::many_files_dirs;
    use futures::compat::Future01CompatExt;
    use maplit::btreemap;
    use mercurial_types::HgChangesetId;
    use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, DateTime};
    use std::str::FromStr;
    use std::sync::Arc;

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
        bcs_id
            .load(ctx.clone(), repo.blobstore())
            .compat()
            .await
            .unwrap()
    }

    #[fbinit::test]
    fn test_do_not_move_anything(fb: FacebookInit) {
        async_unit::tokio_unit_test(async move {
            let (ctx, repo, _hg_cs_id, bcs_id, changeset_args) = prepare(fb).await;
            let newcs = perform_move(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                Arc::new(identity_mover),
                changeset_args,
            )
            .compat()
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
            let newcs = perform_move(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                Arc::new(skip_one),
                changeset_args,
            )
            .compat()
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
            let newcs = perform_move(
                ctx.clone(),
                repo.clone(),
                bcs_id,
                Arc::new(shift_one),
                changeset_args,
            )
            .compat()
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
        hg_cs_id
            .load(ctx.clone(), repo.blobstore())
            .from_err()
            .and_then({
                cloned!(ctx, repo);
                move |hg_cs| {
                    hg_cs
                        .manifestid()
                        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
                        .and_then({
                            cloned!(ctx, repo);
                            move |(path, (file_type, filenode_id))| {
                                filenode_id
                                    .load(ctx.clone(), repo.blobstore())
                                    .from_err()
                                    .map(move |env| (path, (file_type, env.content_id())))
                            }
                        })
                        .collect()
                }
            })
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
                ctx.clone(),
                repo.clone(),
                old_bcs_id,
                Arc::new(shift_one_skip_another),
                changeset_args,
            )
            .compat()
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
}
