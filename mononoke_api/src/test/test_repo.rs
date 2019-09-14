// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{FixedOffset, TimeZone};
use failure::Error;
use fbinit::FacebookInit;
use fixtures::{branch_uneven, linear};

use crate::{ChangesetId, ChangesetSpecifier, CoreContext, HgChangesetId, Mononoke};

#[fbinit::test]
async fn commit_info_by_hash(fb: FacebookInit) -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo(fb))]);
    let ctx = CoreContext::test_mock(fb);
    let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
    let hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let cs_id = ChangesetId::from_str(hash)?;
    let cs = repo
        .changeset(ChangesetSpecifier::Bonsai(cs_id))
        .await?
        .expect("changeset exists");

    assert_eq!(cs.message().await?, "modified 10");
    assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
    assert_eq!(
        cs.author_date().await?,
        FixedOffset::west(7 * 3600).timestamp(1504041761, 0)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_info_by_hg_hash(fb: FacebookInit) -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo(fb))]);
    let ctx = CoreContext::test_mock(fb);
    let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
    let hg_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
    let hg_cs_id = HgChangesetId::from_str(hg_hash)?;
    let cs = repo
        .changeset(ChangesetSpecifier::Hg(hg_cs_id))
        .await?
        .expect("changeset exists");

    let hash = "2cb6d2d3052bfbdd6a95a61f2816d81130033b5f5a99e8d8fc24d9238d85bb48";
    assert_eq!(cs.id(), ChangesetId::from_str(hash)?);
    assert_eq!(cs.hg_id().await?, Some(HgChangesetId::from_str(hg_hash)?));
    assert_eq!(cs.message().await?, "added 3");
    assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
    assert_eq!(
        cs.author_date().await?,
        FixedOffset::west(7 * 3600).timestamp(1504041758, 0)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_info_by_bookmark(fb: FacebookInit) -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo(fb))]);
    let ctx = CoreContext::test_mock(fb);
    let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
    let cs = repo
        .resolve_bookmark("master")
        .await?
        .expect("bookmark exists");

    let hash = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    assert_eq!(cs.id(), ChangesetId::from_str(hash)?);
    let hg_hash = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
    assert_eq!(cs.hg_id().await?, Some(HgChangesetId::from_str(hg_hash)?));
    assert_eq!(cs.message().await?, "modified 10");
    assert_eq!(cs.author().await?, "Jeremy Fitzhardinge <jsgf@fb.com>");
    assert_eq!(
        cs.author_date().await?,
        FixedOffset::west(7 * 3600).timestamp(1504041761, 0)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_hg_changeset_ids(fb: FacebookInit) -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo(fb))]);
    let ctx = CoreContext::test_mock(fb);
    let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
    let hash1 = "2cb6d2d3052bfbdd6a95a61f2816d81130033b5f5a99e8d8fc24d9238d85bb48";
    let hash2 = "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6";
    let hg_hash1 = "607314ef579bd2407752361ba1b0c1729d08b281";
    let hg_hash2 = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
    let ids: HashMap<_, _> = repo
        .changeset_hg_ids(vec![
            ChangesetId::from_str(hash1)?,
            ChangesetId::from_str(hash2)?,
        ])
        .await?
        .into_iter()
        .collect();
    assert_eq!(
        ids.get(&ChangesetId::from_str(hash1)?),
        Some(&HgChangesetId::from_str(hg_hash1)?)
    );
    assert_eq!(
        ids.get(&ChangesetId::from_str(hash2)?),
        Some(&HgChangesetId::from_str(hg_hash2)?)
    );

    Ok(())
}

#[fbinit::test]
async fn commit_is_ancestor_of(fb: FacebookInit) -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), branch_uneven::getrepo(fb))]);
    let ctx = CoreContext::test_mock(fb);
    let repo = mononoke.repo(ctx, "test")?.expect("repo exists");
    let mut changesets = Vec::new();
    for hg_hash in [
        "5d43888a3c972fe68c224f93d41b30e9f888df7c", // 0: branch 1 near top
        "d7542c9db7f4c77dab4b315edd328edf1514952f", // 1: branch 1 near bottom
        "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5", // 2: branch 2
        "15c40d0abc36d47fb51c8eaec51ac7aad31f669c", // 3: base
    ]
    .iter()
    {
        let changeset = repo
            .changeset(ChangesetSpecifier::Hg(HgChangesetId::from_str(hg_hash)?))
            .await
            .expect("changeset exists");
        changesets.push(changeset);
    }
    for (index, base_index, is_ancestor_of) in [
        (0usize, 0usize, true),
        (0, 1, false),
        (0, 2, false),
        (0, 3, false),
        (1, 0, true),
        (1, 1, true),
        (1, 2, false),
        (1, 3, false),
        (2, 0, false),
        (2, 1, false),
        (2, 2, true),
        (2, 3, false),
        (3, 0, true),
        (3, 1, true),
        (3, 2, true),
        (3, 3, true),
    ]
    .iter()
    {
        assert_eq!(
            changesets[*index]
                .as_ref()
                .unwrap()
                .is_ancestor_of(changesets[*base_index].as_ref().unwrap().id())
                .await?,
            *is_ancestor_of,
            "changesets[{}].is_ancestor_of(changesets[{}].id()) == {}",
            *index,
            *base_index,
            *is_ancestor_of
        );
    }
    Ok(())
}
