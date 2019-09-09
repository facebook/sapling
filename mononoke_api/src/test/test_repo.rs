// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::str::FromStr;

use chrono::{FixedOffset, TimeZone};
use failure::Error;
use fixtures::linear;

use crate::{ChangesetId, ChangesetSpecifier, CoreContext, HgChangesetId, Mononoke};

#[tokio::test]
async fn commit_info_by_hash() -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo())]);
    let ctx = CoreContext::test_mock();
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

#[tokio::test]
async fn commit_info_by_hg_hash() -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo())]);
    let ctx = CoreContext::test_mock();
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

#[tokio::test]
async fn commit_info_by_bookmark() -> Result<(), Error> {
    let mononoke = Mononoke::new_test(vec![("test".to_string(), linear::getrepo())]);
    let ctx = CoreContext::test_mock();
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
