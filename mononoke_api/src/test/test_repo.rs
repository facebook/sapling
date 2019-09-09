// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use chrono::{FixedOffset, TimeZone};
use failure::Error;
use fixtures::linear;

use crate::{ChangesetId, ChangesetSpecifier, CoreContext, Mononoke};

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
