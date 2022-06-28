/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::file_history::get_file_history;
use blobstore::Loadable;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::TestRepoFixture;
use manifest::ManifestOps;
use mercurial_types::HgChangesetId;
use mononoke_types::MPath;
use std::str::FromStr;
use tests_utils::resolve_cs_id;

#[fbinit::test]
async fn test_linear_get_file_history(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = Linear::getrepo(fb).await;

    let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
    FilenodesOnlyPublic::derive(&ctx, &repo, master_cs_id).await?;

    let expected_linknodes = vec![
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?,
        HgChangesetId::from_str("607314ef579bd2407752361ba1b0c1729d08b281")?,
        HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0")?,
        HgChangesetId::from_str("cb15ca4a43a59acff5388cea9648c162afde8372")?,
        HgChangesetId::from_str("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b")?,
        HgChangesetId::from_str("0ed509bf086fadcb8a8a5384dc3b550729b0fc17")?,
        HgChangesetId::from_str("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157")?,
        HgChangesetId::from_str("3c15267ebf11807f3d772eb891272b911ec68759")?,
        HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
    ];
    let expected_linknodes = expected_linknodes.into_iter().rev().collect::<Vec<_>>();

    assert_linknodes(
        &ctx,
        &repo,
        expected_linknodes,
        HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?,
        MPath::new("files")?,
        None,
    )
    .await?;

    let expected_linknodes = vec![
        HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
        HgChangesetId::from_str("3c15267ebf11807f3d772eb891272b911ec68759")?,
    ];
    assert_linknodes(
        &ctx,
        &repo,
        expected_linknodes,
        HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?,
        MPath::new("files")?,
        Some(2),
    )
    .await?;

    let expected_linknodes = vec![
        HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?,
        HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
    ];
    assert_linknodes(
        &ctx,
        &repo,
        expected_linknodes,
        HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?,
        MPath::new("10")?,
        None,
    )
    .await?;
    Ok(())
}

async fn assert_linknodes(
    ctx: &CoreContext,
    repo: &BlobRepo,
    expected_linknodes: Vec<HgChangesetId>,
    start_from: HgChangesetId,
    path: MPath,
    max_length: Option<u64>,
) -> Result<(), Error> {
    let root_mf_id = start_from
        .load(ctx, &repo.get_blobstore())
        .await?
        .manifestid();
    let (_, files_hg_id) = root_mf_id
        .find_entry(ctx.clone(), repo.get_blobstore(), Some(path.clone()))
        .await?
        .ok_or_else(|| anyhow!("entry not found"))?
        .into_leaf()
        .ok_or_else(|| anyhow!("expected leaf"))?;

    let history = get_file_history(ctx.clone(), repo.clone(), files_hg_id, path, max_length)
        .await?
        .do_not_handle_disabled_filenodes()?;

    let actual_linknodes = history
        .into_iter()
        .map(|entry| *entry.linknode())
        .collect::<Vec<_>>();

    assert_eq!(expected_linknodes, actual_linknodes);
    Ok(())
}
