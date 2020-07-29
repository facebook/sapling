/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::{FutureExt, TryFutureExt};
use futures_old::future::{err, ok, Future};
use futures_old::stream::Stream;
use manifest::ManifestOps;
use mercurial_types::{HgChangesetId, MPath};
use mononoke_types::ChangesetId;
use slog::info;
use std::collections::{BTreeMap, HashSet};
use std::iter::FromIterator;

use megarepolib::common::{create_save_and_generate_hg_changeset, ChangesetArgs};

fn get_all_files_in_working_copy(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs_id: HgChangesetId,
) -> impl Future<Item = Vec<MPath>, Error = Error> {
    hg_cs_id
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .from_err()
        .and_then({
            cloned!(ctx, repo);
            move |hg_cs| {
                hg_cs
                    .manifestid()
                    .list_leaf_entries(ctx, repo.get_blobstore())
                    .map(|(mpath, _)| mpath)
                    .collect()
            }
        })
}

fn fail_on_path_conflicts(
    ctx: CoreContext,
    repo: BlobRepo,
    hg_cs_id_1: HgChangesetId,
    hg_cs_id_2: HgChangesetId,
) -> impl Future<Item = (), Error = Error> {
    info!(ctx.logger(), "Checking if there are any path conflicts");
    let all_files_1_fut = get_all_files_in_working_copy(ctx.clone(), repo.clone(), hg_cs_id_1);
    let all_files_2_fut = get_all_files_in_working_copy(ctx.clone(), repo.clone(), hg_cs_id_2);
    all_files_1_fut
        .join(all_files_2_fut)
        .and_then(move |(all_files_1, all_files_2)| {
            let all_files_1 = HashSet::<_>::from_iter(all_files_1);
            let all_files_2 = HashSet::from_iter(all_files_2);
            let intersection: Vec<MPath> = all_files_1
                .intersection(&all_files_2)
                .take(10)
                .cloned()
                .collect();
            if intersection.len() > 0 {
                err(format_err!(
                    "There are paths present in both parents: {:?} ...",
                    intersection
                ))
            } else {
                info!(ctx.logger(), "Done checking path conflicts");
                ok(())
            }
        })
}

pub fn perform_merge(
    ctx: CoreContext,
    repo: BlobRepo,
    first_bcs_id: ChangesetId,
    second_bcs_id: ChangesetId,
    resulting_changeset_args: ChangesetArgs,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    let first_hg_cs_id_fut = repo.get_hg_from_bonsai_changeset(ctx.clone(), first_bcs_id.clone());
    let second_hg_cs_id_fut = repo.get_hg_from_bonsai_changeset(ctx.clone(), second_bcs_id.clone());
    first_hg_cs_id_fut
        .join(second_hg_cs_id_fut)
        .and_then({
            cloned!(ctx, repo);
            move |(first_hg_cs_id, second_hg_cs_id)| {
                fail_on_path_conflicts(ctx, repo, first_hg_cs_id, second_hg_cs_id)
            }
        })
        .and_then({
            cloned!(ctx, repo, first_bcs_id, second_bcs_id);
            move |_| {
                info!(
                    ctx.logger(),
                    "Creating a merge bonsai changeset with parents: {:?}, {:?}",
                    first_bcs_id,
                    second_bcs_id
                );
                async move {
                    create_save_and_generate_hg_changeset(
                        &ctx,
                        &repo,
                        vec![first_bcs_id, second_bcs_id],
                        BTreeMap::new(),
                        resulting_changeset_args,
                    )
                    .await
                }
                .boxed()
                .compat()
            }
        })
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use fixtures::merge_even;
    use futures::compat::Future01CompatExt;
    use std::str::FromStr;

    #[fbinit::test]
    fn test_path_conflict_detection(fb: FacebookInit) {
        async_unit::tokio_unit_test(async move {
            let repo = merge_even::getrepo(fb).await;
            let ctx = CoreContext::test_mock(fb);
            let p1 = HgChangesetId::from_str("4f7f3fd428bec1a48f9314414b063c706d9c1aed").unwrap();
            let p2 = HgChangesetId::from_str("16839021e338500b3cf7c9b871c8a07351697d68").unwrap();
            assert!(
                fail_on_path_conflicts(ctx, repo, p1, p2)
                    .compat()
                    .await
                    .is_err(),
                "path conflicts should've been detected"
            );
        });
    }
}
