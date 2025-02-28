/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::DateTime;
use mononoke_types::NonRootMPath;
use mutable_renames::MutableRenameEntry;
use pretty_assertions::assert_eq;
use tests_utils::CreateCommitContext;

use crate::changeset_path::ChangesetPathHistoryContext;
use crate::repo::Repo;
use crate::ChangesetId;
use crate::MononokeRepo;
use crate::RepoContext;

// Generates this commit graph:
//

// o "c2"
// |
// o "c1"
// |
// o   "m1"
// |\
// o | "b3"
// | |
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
// a commits have as many lines in a as the number
// b commits have as many lines in b as the number
// m commits are pure merges without any changes
// c change the number of lines in a and b.
// There are no subdirectories here.
async fn init_repo(
    ctx: &CoreContext,
) -> Result<(RepoContext<Repo>, HashMap<&'static str, ChangesetId>)> {
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
    let mut changesets = HashMap::new();

    changesets.insert(
        "a1",
        CreateCommitContext::new_root(ctx, &repo)
            .add_file("a", "1\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "a2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["a1"]])
            .add_file("a", "2\n1\n")
            .set_author_date(DateTime::from_timestamp(2000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "a3",
        CreateCommitContext::new(ctx, &repo, vec![changesets["a2"]])
            .add_file("a", "2\n1\n3\n")
            .set_author_date(DateTime::from_timestamp(3000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b1",
        CreateCommitContext::new_root(ctx, &repo)
            .add_file("b", "1\n")
            .set_author_date(DateTime::from_timestamp(1500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["b1"]])
            .add_file("b", "1\n2\n")
            .set_author_date(DateTime::from_timestamp(2500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b3",
        CreateCommitContext::new(ctx, &repo, vec![changesets["b2"]])
            .add_file("b", "1\n2\n3\n")
            .set_author_date(DateTime::from_timestamp(3500, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "m1",
        CreateCommitContext::new(ctx, &repo, vec![changesets["b3"], changesets["a3"]])
            .set_author_date(DateTime::from_timestamp(4000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "c1",
        CreateCommitContext::new(ctx, &repo, vec![changesets["m1"]])
            .add_file("a", "2\n1\n3\n4\n")
            .set_author_date(DateTime::from_timestamp(6000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "c2",
        CreateCommitContext::new(ctx, &repo, vec![changesets["c1"]])
            .add_file("b", "4\n1\n2\n3\n")
            .set_author_date(DateTime::from_timestamp(10000, 0)?)
            .commit()
            .await?,
    );

    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[mononoke::fbinit_test]
async fn test_immutable_blame(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    {
        let cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(false).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b2"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    {
        let cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        let a_m1_with_history = cs_m1.path_with_history("a").await?;
        let a_m1_blame = a_m1_with_history.blame(true).await?;
        let a_m1_blame_by_lines: Vec<_> = a_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            a_m1_blame_by_lines,
            vec![
                (changesets["a2"], NonRootMPath::new(b"a")?, 0),
                (changesets["a1"], NonRootMPath::new(b"a")?, 0),
                (changesets["a3"], NonRootMPath::new(b"a")?, 2)
            ]
        );
    }
    Ok(())
}

async fn add_mutable_rename<R: MononokeRepo>(
    src: &ChangesetPathHistoryContext<R>,
    dst: &ChangesetPathHistoryContext<R>,
) -> Result<()> {
    let repo = src.repo_ctx();
    let mutable_renames = repo.mutable_renames();

    let src_unode = src.unode_id().await?.expect("No source unode");

    let rename_entry = MutableRenameEntry::new(
        dst.changeset().id(),
        dst.path().clone(),
        src.changeset().id(),
        src.path().clone(),
        src_unode,
    )?;

    mutable_renames
        .add_or_overwrite_renames(repo.ctx(), repo.commit_graph(), vec![rename_entry])
        .await
}

#[mononoke::fbinit_test]
async fn test_linear_mutable_blame(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Add a mutable rename cutting out b2, so b3 goes straight to b1
    {
        let cs_b3 = repo
            .changeset(changesets["b3"])
            .await?
            .expect("changeset exists");
        let b_b3_with_history = cs_b3.path_with_history("b").await?;

        let cs_b1 = repo
            .changeset(changesets["b1"])
            .await?
            .expect("changeset exists");
        let b_b1_with_history = cs_b1.path_with_history("b").await?;
        add_mutable_rename(&b_b1_with_history, &b_b3_with_history).await?;
    }

    // Check that immutable blame isn't changed
    {
        let cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(false).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b2"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    // Check it works direct
    {
        let mut cs_b3 = repo
            .changeset(changesets["b3"])
            .await?
            .expect("changeset exists");
        cs_b3
            .add_mutable_renames(vec![NonRootMPath::new(b"b")?.into()].into_iter())
            .await?;
        let b_b3_with_history = cs_b3.path_with_history("b").await?;
        let b_b3_blame = b_b3_with_history.blame(true).await?;
        let b_b3_blame_by_lines: Vec<_> = b_b3_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_b3_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b3"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    // And from a descendant
    {
        let mut cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        cs_m1
            .add_mutable_renames(vec![NonRootMPath::new(b"b")?.into()].into_iter())
            .await?;
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(true).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b3"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_merge_commit_mutable_blame(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Add a mutable rename sending b3 to a2, and one sending a3 to b2
    // This makes the commit tree (with : for immutable changes overridden by
    // mutable renames) look like:
    //     m1
    //    /\
    //   /  \
    //  a3   b3
    //  : \/ :
    //  : /\ :
    //  a2   b2
    //  |    |
    //  a1   b1
    // Note the cross in the middle - if you only look at mutable renames,
    // you get:
    //     m1
    //    /\
    //   /  \
    //  a3   b3
    //  |    |
    //  |    |
    //  b2   a2
    //  |    |
    //  b1   a1
    // swapping chains over
    {
        let cs_b3 = repo
            .changeset(changesets["b3"])
            .await?
            .expect("changeset exists");
        let b_b3_with_history = cs_b3.path_with_history("b").await?;

        let cs_a2 = repo
            .changeset(changesets["a2"])
            .await?
            .expect("changeset exists");
        let a_a2_with_history = cs_a2.path_with_history("a").await?;
        add_mutable_rename(&a_a2_with_history, &b_b3_with_history).await?;
    }
    {
        let cs_a3 = repo
            .changeset(changesets["a3"])
            .await?
            .expect("changeset exists");
        let a_a3_with_history = cs_a3.path_with_history("a").await?;
        let cs_b2 = repo
            .changeset(changesets["b2"])
            .await?
            .expect("changeset exists");
        let b_b2_with_history = cs_b2.path_with_history("b").await?;
        add_mutable_rename(&b_b2_with_history, &a_a3_with_history).await?;
    }

    // Check that immutable blame isn't changed
    {
        let cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(false).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b2"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }
    {
        let cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        let a_m1_with_history = cs_m1.path_with_history("a").await?;
        let a_m1_blame = a_m1_with_history.blame(false).await?;
        let a_m1_blame_by_lines: Vec<_> = a_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            a_m1_blame_by_lines,
            vec![
                (changesets["a2"], NonRootMPath::new(b"a")?, 0),
                (changesets["a1"], NonRootMPath::new(b"a")?, 0),
                (changesets["a3"], NonRootMPath::new(b"a")?, 2)
            ]
        );
    }

    // Check it works direct
    {
        let mut cs_b3 = repo
            .changeset(changesets["b3"])
            .await?
            .expect("changeset exists");
        cs_b3
            .add_mutable_renames(vec![NonRootMPath::new(b"b")?.into()].into_iter())
            .await?;
        let b_b3_with_history = cs_b3.path_with_history("b").await?;
        let b_b3_blame = b_b3_with_history.blame(true).await?;
        let b_b3_blame_by_lines: Vec<_> = b_b3_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_b3_blame_by_lines,
            vec![
                (changesets["a1"], NonRootMPath::new(b"a")?, 0),
                (changesets["b3"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    // And from a descendant
    {
        let mut cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        cs_m1
            .add_mutable_renames(vec![NonRootMPath::new(b"b")?.into()].into_iter())
            .await?;
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(true).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["a1"], NonRootMPath::new(b"a")?, 0),
                (changesets["b3"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_two_entry_mutable_blame(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Add a mutable rename cutting out b2, so b3 goes straight to b1,
    // and a second one taking b2 to a1 that should be inert
    {
        let cs_b3 = repo
            .changeset(changesets["b3"])
            .await?
            .expect("changeset exists");
        let b_b3_with_history = cs_b3.path_with_history("b").await?;

        let cs_b1 = repo
            .changeset(changesets["b1"])
            .await?
            .expect("changeset exists");
        let b_b1_with_history = cs_b1.path_with_history("b").await?;
        add_mutable_rename(&b_b1_with_history, &b_b3_with_history).await?;

        let cs_b2 = repo
            .changeset(changesets["b2"])
            .await?
            .expect("changeset exists");
        let b_b2_with_history = cs_b2.path_with_history("b").await?;
        let cs_a1 = repo
            .changeset(changesets["a1"])
            .await?
            .expect("changeset exists");
        let a_a1_with_history = cs_a1.path_with_history("a").await?;
        add_mutable_rename(&a_a1_with_history, &b_b2_with_history).await?;
    }

    // Check that immutable blame isn't changed
    {
        let cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(false).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b2"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    // Check it works direct
    {
        let mut cs_b3 = repo
            .changeset(changesets["b3"])
            .await?
            .expect("changeset exists");
        cs_b3
            .add_mutable_renames(vec![NonRootMPath::new(b"b")?.into()].into_iter())
            .await?;
        let b_b3_with_history = cs_b3.path_with_history("b").await?;
        let b_b3_blame = b_b3_with_history.blame(true).await?;
        let b_b3_blame_by_lines: Vec<_> = b_b3_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_b3_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b3"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    // And from a descendant
    {
        let mut cs_m1 = repo
            .changeset(changesets["m1"])
            .await?
            .expect("changeset exists");
        cs_m1
            .add_mutable_renames(vec![NonRootMPath::new(b"b")?.into()].into_iter())
            .await?;
        let b_m1_with_history = cs_m1.path_with_history("b").await?;
        let b_m1_blame = b_m1_with_history.blame(true).await?;
        let b_m1_blame_by_lines: Vec<_> = b_m1_blame
            .lines()?
            .map(|line| (*line.changeset_id, line.path.clone(), line.origin_offset))
            .collect();

        assert_eq!(
            b_m1_blame_by_lines,
            vec![
                (changesets["b1"], NonRootMPath::new(b"b")?, 0),
                (changesets["b3"], NonRootMPath::new(b"b")?, 1),
                (changesets["b3"], NonRootMPath::new(b"b")?, 2)
            ]
        );
    }

    Ok(())
}
