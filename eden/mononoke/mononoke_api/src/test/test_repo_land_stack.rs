/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Freshness;
use bookmarks_movement::BookmarkKindRestrictions::AnyKind;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use hooks::PushAuthoredBy::User;
use maplit::hashset;
use mononoke_types::ChangesetId;
use tests_utils::drawdag::create_from_dag;

use crate::repo::BookmarkFreshness;
use crate::repo::Repo;
use crate::repo::RepoContext;

async fn init_repo(ctx: &CoreContext) -> Result<(RepoContext, BTreeMap<String, ChangesetId>)> {
    let blob_repo: BlobRepo = test_repo_factory::build_empty(ctx.fb)?;
    let changesets = create_from_dag(
        ctx,
        &blob_repo,
        r##"
            A-B-C-G
             \ \
              \ F
               D-E
        "##,
    )
    .await?;
    let mut txn = blob_repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(
        &BookmarkName::new("trunk")?,
        changesets["C"],
        BookmarkUpdateReason::TestMove,
    )?;
    txn.commit().await?;

    let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::test]
async fn land_stack(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Land G - it should be rewritten even though its parent is C.
    let outcome = repo
        .land_stack(
            "trunk",
            changesets["G"],
            changesets["C"],
            None,
            AnyKind,
            User,
        )
        .await?;
    let trunk_g = repo
        .resolve_bookmark("trunk", BookmarkFreshness::MostRecent)
        .await?
        .expect("trunk should be set");
    assert_eq!(trunk_g.id(), outcome.head);
    assert_ne!(trunk_g.id(), changesets["G"]);
    assert_eq!(outcome.rebased_changesets[0].id_old, changesets["G"]);
    assert_eq!(outcome.rebased_changesets[0].id_new, trunk_g.id());

    // Land D and E, both commits should get mapped
    let outcome = repo
        .land_stack(
            "trunk",
            changesets["E"],
            changesets["A"],
            None,
            AnyKind,
            User,
        )
        .await?;
    let trunk_e = repo
        .resolve_bookmark("trunk", BookmarkFreshness::MostRecent)
        .await?
        .expect("trunk should be set");
    assert_eq!(trunk_e.id(), outcome.head);
    let mapping: HashMap<_, _> = outcome
        .rebased_changesets
        .iter()
        .map(|pair| (pair.id_old, pair.id_new))
        .collect();
    assert_eq!(mapping.len(), 2);
    assert_ne!(mapping[&changesets["D"]], changesets["D"]);
    assert_ne!(mapping[&changesets["E"]], changesets["D"]);
    assert_ne!(mapping[&changesets["D"]], trunk_e.id());
    assert_eq!(mapping[&changesets["E"]], trunk_e.id());

    // Land F, its parent should be the landed version of E
    let outcome = repo
        .land_stack(
            "trunk",
            changesets["F"],
            changesets["B"],
            None,
            AnyKind,
            User,
        )
        .await?;
    let trunk_f = repo
        .resolve_bookmark("trunk", BookmarkFreshness::MostRecent)
        .await?
        .expect("trunk should be set");
    assert_eq!(trunk_f.id(), outcome.head);
    assert_eq!(trunk_f.parents().await?, vec![trunk_e.id()]);

    // With everything landed, all files should be present
    let files: HashSet<_> = trunk_f
        .path_with_content("")
        .await?
        .tree()
        .await?
        .expect("root must be a tree")
        .list()
        .await?
        .map(|(name, _entry)| name)
        .collect();
    let expected_files: HashSet<_> = hashset! {"A", "B", "C", "D", "E", "F", "G"}
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(files, expected_files);

    // Check the bookmark moves created BookmarkLogUpdate entries
    let entries = repo
        .blob_repo()
        .bookmark_update_log()
        .list_bookmark_log_entries(
            ctx.clone(),
            BookmarkName::new("trunk")?,
            4,
            None,
            Freshness::MostRecent,
        )
        .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(
        entries,
        vec![
            (Some(trunk_f.id()), BookmarkUpdateReason::Pushrebase),
            (Some(trunk_e.id()), BookmarkUpdateReason::Pushrebase),
            (Some(trunk_g.id()), BookmarkUpdateReason::Pushrebase),
            (Some(changesets["C"]), BookmarkUpdateReason::TestMove),
        ]
    );

    Ok(())
}
