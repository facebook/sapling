/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_graph_testlib::utils::assert_topological_order;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use maplit::hashset;
use mononoke_macros::mononoke;
use mononoke_types::DateTime;
use tests_utils::CreateCommitContext;

use crate::ChangesetHistoryOptions;
use crate::ChangesetId;
use crate::ChangesetLinearHistoryOptions;
use crate::ChangesetPathHistoryOptions;
use crate::RepoContext;
use crate::repo::Repo;

// Generates this commit graph:
//
// @ "c2"
// |
// o   "m2"
// |\
// | o "e3"
// | |
// | o "b3"
// | |
// o | "e2"
// | |
// o | "a4"
// |/
// o "c1"
// |
// o "e1"
// |
// o   "m1"
// |\
// o | "b2"
// | |
// o | "b1"
//   |
//   o "a3"
//   |
//   o "a2"
//   |
//   o "a1"
//
// Commits e1, e2 and e3 are empty (contain no file changes).
async fn init_repo(
    ctx: &CoreContext,
) -> Result<(RepoContext<Repo>, HashMap<&'static str, ChangesetId>)> {
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
    let mut changesets = HashMap::new();

    changesets.insert(
        "a1",
        CreateCommitContext::new_root(ctx, &repo)
            .add_file("a", "1")
            .add_file("aa", "11")
            .add_file("aaa", "111")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "a2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["a1"]])
            .add_file("a", "2")
            .add_file("dir1/a", "2")
            .add_file_with_copy_info("renamed_aa", "22", (changesets["a1"], "aa"))
            .delete_file("aa")
            .add_file_with_copy_info("copied_aaa", "222", (changesets["a1"], "aaa"))
            .set_author_date(DateTime::from_timestamp(2000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "a3",
        CreateCommitContext::new(ctx, &repo, vec![changesets["a2"]])
            .add_file("a", "3")
            .add_file("dir1/a", "3")
            .add_file("dir3/a", "3")
            .add_file("renamed_aa", "33")
            .add_file("copied_aaa", "333")
            .set_author_date(DateTime::from_timestamp(3000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b1",
        CreateCommitContext::new_root(ctx, &repo)
            .add_file("b", "1")
            .add_file("dir2/b", "1")
            .set_author_date(DateTime::from_timestamp(1500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["b1"]])
            .add_file("b", "2")
            .add_file("dir2/b", "2")
            .add_file("dir3/b", "2")
            .set_author_date(DateTime::from_timestamp(2500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "m1",
        CreateCommitContext::new(ctx, &repo, vec![changesets["b2"], changesets["a3"]])
            .add_file("a", "3")
            .add_file("dir1/a", "3")
            .add_file("dir3/a", "3")
            .add_file("b", "2")
            .add_file("dir2/b", "2")
            .add_file("dir3/b", "2")
            .set_author_date(DateTime::from_timestamp(4000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "e1",
        CreateCommitContext::new(ctx, &repo, vec![changesets["m1"]])
            .set_author_date(DateTime::from_timestamp(5000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "c1",
        CreateCommitContext::new(ctx, &repo, vec![changesets["e1"]])
            .add_file("c", "1")
            .add_file("dir3/c", "1")
            .set_author_date(DateTime::from_timestamp(6000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "a4",
        CreateCommitContext::new(ctx, &repo, vec![changesets["c1"]])
            .add_file("a", "4")
            .add_file("dir1/a", "4")
            .add_file("dir3/a", "4")
            .set_author_date(DateTime::from_timestamp(7000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "e2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["a4"]])
            .set_author_date(DateTime::from_timestamp(8000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b3",
        CreateCommitContext::new(ctx, &repo, vec![changesets["c1"]])
            .add_file("b", "3")
            .add_file("dir2/b", "3")
            .add_file("dir3/b", "3")
            .set_author_date(DateTime::from_timestamp(7500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "e3",
        CreateCommitContext::new(ctx, &repo, vec![changesets["b3"]])
            .set_author_date(DateTime::from_timestamp(8500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "m2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["e2"], changesets["e3"]])
            .add_file("a", "4")
            .add_file("dir1/a", "4")
            .add_file("dir3/a", "4")
            .add_file("b", "3")
            .add_file("dir2/b", "3")
            .add_file("dir3/b", "3")
            .set_author_date(DateTime::from_timestamp(9000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "c2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["m2"]])
            .add_file("c", "2")
            .add_file("dir3/c", "2")
            .set_author_date(DateTime::from_timestamp(10000, 0)?)
            .commit()
            .await?,
    );

    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[mononoke::fbinit_test]
async fn commit_path_history(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");

    // History of file "a" includes commits that modified "a".
    let a_path = cs.path_with_history("a").await?;
    let a_history: Vec<_> = a_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        a_history,
        vec![
            changesets["a4"],
            changesets["m1"],
            changesets["a3"],
            changesets["a2"],
            changesets["a1"],
        ]
    );

    // History of file "renamed_aa" doesn't include commits before the rename.
    let renamed_aa_path = cs.path_with_history("renamed_aa").await?;
    let renamed_aa_history: Vec<_> = renamed_aa_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        renamed_aa_history,
        vec![
            changesets["a3"], // This commit modified renamed_aa
            changesets["a2"], // This commit renamed aa to renamed_aa
        ]
    );

    // History of deleted file "aa"
    let aa_path = cs.path_with_history("aa").await?;
    let aa_history: Vec<_> = aa_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        aa_history,
        vec![
            changesets["a2"], // This commit renamed aa to renamed_aa
            changesets["a1"], // This commit added aa
        ]
    );

    // History of file "copied_aaa" doesn't include commits before the copy.
    let copied_aaa_path = cs.path_with_history("copied_aaa").await?;
    let copied_aaa_history: Vec<_> = copied_aaa_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        copied_aaa_history,
        vec![
            changesets["a3"], // This commit modified copied_aaa
            changesets["a2"], // This commit copied aaa to copied_aaa
        ]
    );

    // History of directory "dir2" includes commits that modified "dir2/b".
    let dir2_path = cs.path_with_history("dir2").await?;
    let dir2_history: Vec<_> = dir2_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        dir2_history,
        vec![
            changesets["b3"],
            changesets["m1"],
            changesets["b2"],
            changesets["b1"],
        ]
    );

    // History of directory "dir3" includes some commits on all branches.
    let dir3_path = cs.path_with_history("dir3").await?;
    let dir3_history: Vec<_> = dir3_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        dir3_history,
        vec![
            changesets["c2"],
            changesets["m2"],
            changesets["a4"],
            changesets["b3"],
            changesets["c1"],
            changesets["m1"],
            changesets["a3"],
            changesets["b2"],
        ]
    );

    // Root path history includes all commits except the empty ones.
    let root_path = cs.path_with_history("").await?;
    let root_history: Vec<_> = root_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        root_history,
        vec![
            changesets["c2"],
            changesets["m2"],
            changesets["a4"],
            changesets["b3"],
            changesets["c1"],
            changesets["m1"],
            changesets["a3"],
            changesets["b2"],
            changesets["a2"],
            changesets["a1"],
            changesets["b1"],
        ]
    );

    // Setting until_timestamp omits some commits.
    let a_history_with_time_filter: Vec<_> = a_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                until_timestamp: Some(2500),
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        a_history_with_time_filter,
        vec![changesets["a4"], changesets["m1"], changesets["a3"],]
    );

    // Setting descendants_of omits more commits.
    let a_history_with_descendants_of: Vec<_> = a_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                descendants_of: Some(changesets["b1"]),
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        a_history_with_descendants_of,
        vec![changesets["a4"], changesets["m1"]]
    );

    let a_history_with_exclusion: Vec<_> = a_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                exclude_changeset_and_ancestors: Some(changesets["a3"]),
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        a_history_with_exclusion,
        vec![changesets["a4"], changesets["m1"]]
    );

    let a_history_with_exclusion: Vec<_> = a_path
        .history(
            &ctx,
            ChangesetPathHistoryOptions {
                exclude_changeset_and_ancestors: Some(changesets["b1"]),
                until_timestamp: Some(2500),
                follow_history_across_deletions: true,
                ..Default::default()
            },
        )
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_eq!(
        a_history_with_exclusion,
        vec![changesets["a4"], changesets["m1"], changesets["a3"]]
    );

    Ok(())
}

async fn assert_history(
    ctx: &CoreContext,
    commit_graph: &CommitGraph,
    history: Vec<ChangesetId>,
    expected_cs_ids: HashSet<ChangesetId>,
) -> Result<()> {
    let history = history.into_iter().rev().collect();
    assert_topological_order(commit_graph, ctx, &history).await?;
    assert_eq!(history.into_iter().collect::<HashSet<_>>(), expected_cs_ids);

    Ok(())
}

#[mononoke::fbinit_test]
async fn commit_history(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");

    // The commit history includes all commits, including empty ones.
    let history: Vec<_> = cs
        .history(Default::default())
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["e3"],
            changesets["a4"],
            changesets["b3"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
            changesets["a3"],
            changesets["b1"],
            changesets["a2"],
            changesets["a1"],
        },
    )
    .await?;

    // The commit history of an empty commit starts with itself.
    let cs = repo
        .changeset(changesets["e1"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .history(Default::default())
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
            changesets["a3"],
            changesets["b1"],
            changesets["a2"],
            changesets["a1"],
        },
    )
    .await?;

    // Setting until_timestamp omits some commits.
    let history: Vec<_> = cs
        .history(ChangesetHistoryOptions {
            until_timestamp: Some(2500),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
            changesets["a3"],
        },
    )
    .await?;

    // Setting descendendants_of omits some commits.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .history(ChangesetHistoryOptions {
            descendants_of: Some(changesets["b2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["e3"],
            changesets["a4"],
            changesets["b3"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
        },
    )
    .await?;

    // Setting exclude_changeset omits some commits.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .history(ChangesetHistoryOptions {
            until_timestamp: Some(2500),
            exclude_changeset_and_ancestors: Some(changesets["b2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["e3"],
            changesets["a4"],
            changesets["b3"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
            changesets["a3"],
        },
    )
    .await?;

    let cs = repo
        .changeset(changesets["m2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .history(ChangesetHistoryOptions {
            exclude_changeset_and_ancestors: Some(changesets["c2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(&ctx, repo.repo().commit_graph(), history, hashset! {}).await?;

    // Setting both descendants_of and exclude_changeset_and_ancestors
    // lets us filter out the descendant.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .history(ChangesetHistoryOptions {
            descendants_of: Some(changesets["b2"]),
            exclude_changeset_and_ancestors: Some(changesets["b2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["e3"],
            changesets["a4"],
            changesets["b3"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
        },
    )
    .await?;

    Ok(())
}

#[mononoke::fbinit_test]
async fn commit_linear_history(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");

    // The commit history includes all commits, including empty ones.
    let history: Vec<_> = cs
        .linear_history(Default::default())
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["a4"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
            changesets["b1"],
        },
    )
    .await?;

    // The commit history of an empty commit starts with itself.
    let cs = repo
        .changeset(changesets["e1"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .linear_history(Default::default())
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
            changesets["b1"],
        },
    )
    .await?;

    // Setting descendendants_of omits some commits.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .linear_history(ChangesetLinearHistoryOptions {
            descendants_of: Some(changesets["b2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["a4"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
            changesets["b2"],
        },
    )
    .await?;

    // Setting exclude_changeset omits some commits.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .linear_history(ChangesetLinearHistoryOptions {
            exclude_changeset_and_ancestors: Some(changesets["b2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["a4"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
        },
    )
    .await?;

    let cs = repo
        .changeset(changesets["m2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .linear_history(ChangesetLinearHistoryOptions {
            exclude_changeset_and_ancestors: Some(changesets["c2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(&ctx, repo.repo().commit_graph(), history, hashset! {}).await?;

    // Setting both descendants_of and exclude_changeset_and_ancestors
    // lets us filter out the descendant.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .linear_history(ChangesetLinearHistoryOptions {
            descendants_of: Some(changesets["b2"]),
            exclude_changeset_and_ancestors: Some(changesets["b2"]),
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["c2"],
            changesets["m2"],
            changesets["e2"],
            changesets["a4"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
        },
    )
    .await?;

    // Setting skip parameter to skip the first three commits.
    let cs = repo
        .changeset(changesets["c2"])
        .await?
        .expect("changeset exists");
    let history: Vec<_> = cs
        .linear_history(ChangesetLinearHistoryOptions {
            descendants_of: Some(changesets["b2"]),
            exclude_changeset_and_ancestors: Some(changesets["b2"]),
            skip: 3,
            ..Default::default()
        })
        .await?
        .and_then(|cs| async move { Ok(cs.id()) })
        .try_collect()
        .await?;
    assert_history(
        &ctx,
        repo.repo().commit_graph(),
        history,
        hashset! {
            changesets["a4"],
            changesets["c1"],
            changesets["e1"],
            changesets["m1"],
        },
    )
    .await?;

    Ok(())
}
