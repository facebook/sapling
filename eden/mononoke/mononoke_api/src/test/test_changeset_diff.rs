/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use fbinit::FacebookInit;
use fixtures::ManyFilesDirs;
use fixtures::TestRepoFixture;
use maplit::btreeset;
use pretty_assertions::assert_eq;

use crate::ChangesetDiffItem;
use crate::ChangesetFileOrdering;
use crate::ChangesetPathDiffContext;
use crate::CoreContext;
use crate::HgChangesetId;
use crate::Mononoke;
use crate::MononokePath;
use tests_utils::CreateCommitContext;

#[fbinit::test]
async fn test_diff_with_moves(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;
    let root = CreateCommitContext::new_root(&ctx, &blobrepo)
        .add_file("file_to_move", "context1")
        .commit()
        .await?;

    let commit_with_move = CreateCommitContext::new(&ctx, &blobrepo, vec![root])
        .add_file_with_copy_info("file_moved", "context", (root, "file_to_move"))
        .delete_file("file_to_move")
        .commit()
        .await?;

    let mononoke =
        Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_with_move_ctx = repo
        .changeset(commit_with_move)
        .await?
        .ok_or(anyhow!("commit not found"))?;
    let diff = commit_with_move_ctx
        .diff_unordered(
            &repo.changeset(root).await?.context("commit not found")?,
            true, /* include_copies_renames */
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
        )
        .await?;

    assert_eq!(diff.len(), 1);
    match diff.get(0) {
        Some(ChangesetPathDiffContext::Moved(to, from)) => {
            assert_eq!(to.path(), &MononokePath::try_from("file_moved")?);
            assert_eq!(from.path(), &MononokePath::try_from("file_to_move")?);
        }
        _ => {
            panic!("unexpected diff");
        }
    }
    Ok(())
}

#[fbinit::test]
async fn test_diff_with_multiple_copies(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;
    let root = CreateCommitContext::new_root(&ctx, &blobrepo)
        .add_file("file_to_copy", "context1")
        .commit()
        .await?;

    let commit_with_copies = CreateCommitContext::new(&ctx, &blobrepo, vec![root])
        .add_file_with_copy_info("copy_one", "context", (root, "file_to_copy"))
        .add_file_with_copy_info("copy_two", "context", (root, "file_to_copy"))
        .commit()
        .await?;

    let mononoke =
        Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_with_copies_ctx = repo
        .changeset(commit_with_copies)
        .await?
        .ok_or(anyhow!("commit not found"))?;
    let diff = commit_with_copies_ctx
        .diff_unordered(
            &repo.changeset(root).await?.context("commit not found")?,
            true, /* include_copies_renames */
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
        )
        .await?;

    assert_eq!(diff.len(), 2);
    match diff.get(0) {
        Some(ChangesetPathDiffContext::Copied(to, from)) => {
            assert_eq!(to.path(), &MononokePath::try_from("copy_one")?);
            assert_eq!(from.path(), &MononokePath::try_from("file_to_copy")?);
        }
        other => panic!("unexpected diff: {:?}", other),
    }
    match diff.get(1) {
        Some(ChangesetPathDiffContext::Copied(to, from)) => {
            assert_eq!(to.path(), &MononokePath::try_from("copy_two")?);
            assert_eq!(from.path(), &MononokePath::try_from("file_to_copy")?);
        }
        other => panic!("unexpected diff: {:?}", other),
    }
    Ok(())
}

#[fbinit::test]
async fn test_diff_with_multiple_moves(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;
    let root = CreateCommitContext::new_root(&ctx, &blobrepo)
        .add_file("file_to_move", "context1")
        .commit()
        .await?;

    let commit_with_moves = CreateCommitContext::new(&ctx, &blobrepo, vec![root])
        .add_file_with_copy_info("copy_one", "context", (root, "file_to_move"))
        .add_file_with_copy_info("copy_two", "context", (root, "file_to_move"))
        .add_file_with_copy_info("copy_zzz", "context", (root, "file_to_move"))
        .delete_file("file_to_move")
        .commit()
        .await?;

    let mononoke =
        Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_with_moves_ctx = repo
        .changeset(commit_with_moves)
        .await?
        .ok_or(anyhow!("commit not found"))?;
    let diff = commit_with_moves_ctx
        .diff_unordered(
            &repo.changeset(root).await?.context("commit not found")?,
            true, /* include_copies_renames */
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
        )
        .await?;

    assert_eq!(diff.len(), 3);
    // The first copy of the file becomes a move.
    match diff.get(0) {
        Some(ChangesetPathDiffContext::Moved(to, from)) => {
            assert_eq!(to.path(), &MononokePath::try_from("copy_one")?);
            assert_eq!(from.path(), &MononokePath::try_from("file_to_move")?);
        }
        other => panic!("unexpected diff: {:?}", other),
    }
    match diff.get(1) {
        Some(ChangesetPathDiffContext::Copied(to, from)) => {
            assert_eq!(to.path(), &MononokePath::try_from("copy_two")?);
            assert_eq!(from.path(), &MononokePath::try_from("file_to_move")?);
        }
        other => panic!("unexpected diff: {:?}", other),
    }
    match diff.get(2) {
        Some(ChangesetPathDiffContext::Copied(to, from)) => {
            assert_eq!(to.path(), &MononokePath::try_from("copy_zzz")?);
            assert_eq!(from.path(), &MononokePath::try_from("file_to_move")?);
        }
        other => panic!("unexpected diff: {:?}", other),
    }
    Ok(())
}

fn check_root_dir_diff(diff: Option<&ChangesetPathDiffContext>) -> Result<(), Error> {
    match diff {
        Some(ChangesetPathDiffContext::Changed(path1, path2)) if path1.path() == path2.path() => {
            assert_eq!(path1.path(), &MononokePath::try_from("")?);
        }
        _ => {
            panic!("unexpected root dir diff")
        }
    }
    Ok(())
}
#[fbinit::test]
async fn test_diff_with_dirs(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(
        ctx.clone(),
        vec![("test".to_string(), ManyFilesDirs::getrepo(fb).await)],
    )
    .await?;
    let repo = mononoke
        .repo(ctx, "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;

    // Case one: dirs added
    let cs_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4")?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");
    let other_cs_id = HgChangesetId::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8")?;
    let other_cs = repo
        .changeset(other_cs_id)
        .await?
        .expect("other changeset exists");

    let diff: Vec<_> = cs
        .diff_unordered(&other_cs, false, None, btreeset! {ChangesetDiffItem::TREES})
        .await?;
    assert_eq!(diff.len(), 6);
    check_root_dir_diff(diff.get(0))?;
    match diff.get(1) {
        Some(ChangesetPathDiffContext::Added(path)) => {
            assert_eq!(path.path(), &MononokePath::try_from("dir2")?);
        }
        _ => {
            panic!("unexpected diff");
        }
    }

    // Case two: dir (with subdirs) replaced with file
    let cs_id = HgChangesetId::from_str("051946ed218061e925fb120dac02634f9ad40ae2")?;
    let cs = repo.changeset(cs_id).await?.expect("changeset exists");
    let other_cs_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4")?;
    let other_cs = repo
        .changeset(other_cs_id)
        .await?
        .expect("other changeset exists");

    // Added
    let diff: Vec<_> = cs
        .diff_unordered(&other_cs, false, None, btreeset! {ChangesetDiffItem::TREES})
        .await?;
    assert_eq!(diff.len(), 5);
    check_root_dir_diff(diff.get(0))?;
    match diff.get(1) {
        Some(ChangesetPathDiffContext::Removed(path)) => {
            assert_eq!(path.path(), &MononokePath::try_from("dir1")?);
        }
        _ => {
            panic!("unexpected diff");
        }
    }

    Ok(())
}

fn check_diff_paths(diff_ctxs: &[ChangesetPathDiffContext], paths: &[&str]) {
    let diff_paths = diff_ctxs
        .iter()
        .map(|diff_ctx| match diff_ctx {
            ChangesetPathDiffContext::Added(file) | ChangesetPathDiffContext::Removed(file) => {
                file.path().to_string()
            }
            ChangesetPathDiffContext::Changed(to, from) => {
                assert_eq!(
                    from.path(),
                    to.path(),
                    "paths for changed file do not match"
                );
                to.path().to_string()
            }
            ChangesetPathDiffContext::Copied(to, from)
            | ChangesetPathDiffContext::Moved(to, from) => {
                assert_ne!(
                    from.path(),
                    to.path(),
                    "paths for copied or moved file should not match"
                );
                // Use the destination path, as this is where it should appear
                // in the ordering.
                to.path().to_string()
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(diff_paths, paths,);
}

#[fbinit::test]
async fn test_ordered_diff(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;
    let root = CreateCommitContext::new_root(&ctx, &blobrepo)
        .add_file("root", "root")
        .commit()
        .await?;

    // List of file names to test in repo order.  Note in particular that
    // "j.txt" is after "j/k" even though "." is before "/" in lexicographic
    // order, as we sort based on the directory name ("j").
    let file_list = [
        "!", "0", "1", "10", "2", "a/a/a/a", "a/a/a/b", "d/e", "d/g", "i", "j/k", "j.txt", "p",
        "r/s/t/u", "r/v", "r/w/x", "r/y", "z", "é",
    ];

    let mut commit = CreateCommitContext::new(&ctx, &blobrepo, vec![root]);
    for file in file_list.iter() {
        commit = commit.add_file(*file, *file);
    }
    let commit = commit.commit().await?;

    let mononoke =
        Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_ctx = repo
        .changeset(commit)
        .await?
        .ok_or(anyhow!("commit not found"))?;
    let root_ctx = &repo.changeset(root).await?.context("commit not found")?;
    let diff = commit_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            None,
        )
        .await?;

    check_diff_paths(&diff, &file_list);

    // Test limits and continuation.
    let diff = commit_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            Some(8),
        )
        .await?;
    check_diff_paths(&diff, &file_list[..8]);
    let diff = commit_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(file_list[7].try_into()?),
            },
            Some(8),
        )
        .await?;
    check_diff_paths(&diff, &file_list[8..16]);
    let diff = commit_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(file_list[15].try_into()?),
            },
            Some(8),
        )
        .await?;
    check_diff_paths(&diff, &file_list[16..]);

    let mod_file_list = [
        "1", "d/e", "d/g", "i/ii", "j/k", "j.txt", "p", "r/v", "r/w/y", "z",
    ];
    let del_file_list = ["10", "i", "r/w/x", "r/y"];

    let mut all_file_list = mod_file_list
        .iter()
        .chain(del_file_list.iter())
        .map(Deref::deref)
        .collect::<Vec<_>>();
    all_file_list.sort_unstable();

    let mut commit2 = CreateCommitContext::new(&ctx, &blobrepo, vec![commit]);
    for file in mod_file_list.iter() {
        commit2 = commit2.add_file(*file, "modified");
    }
    commit2 = commit2
        .add_file_with_copy_info("d/f", "copied", (commit, "d/g"))
        .add_file_with_copy_info("q/y", "moved", (commit, "r/y"));
    for file in del_file_list.iter() {
        commit2 = commit2.delete_file(*file);
    }
    let commit2 = commit2.commit().await?;

    let commit2_ctx = repo
        .changeset(commit2)
        .await?
        .ok_or(anyhow!("commit not found"))?;
    let diff = commit2_ctx
        .diff(
            &commit_ctx,
            true, /* include_copies_renames */
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            None,
        )
        .await?;

    let all_file_list = [
        "1", "10", "d/e", "d/f", "d/g", "i", "i/ii", "j/k", "j.txt", "p", "q/y", "r/v", "r/w/x",
        "r/w/y", "z",
    ];
    check_diff_paths(&diff, &all_file_list);

    // Diff including trees.
    let diff = commit2_ctx
        .diff(
            &commit_ctx,
            true, /* include_copies_renames */
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES, ChangesetDiffItem::TREES},
            ChangesetFileOrdering::Ordered { after: None },
            None,
        )
        .await?;

    // "i" is listed twice as a file that is deleted and a tree that is added
    let all_file_and_dir_list = [
        "", "1", "10", "d", "d/e", "d/f", "d/g", "i", "i", "i/ii", "j", "j/k", "j.txt", "p", "q",
        "q/y", "r", "r/v", "r/w", "r/w/x", "r/w/y", "z",
    ];
    check_diff_paths(&diff, &all_file_and_dir_list);

    // Diff over two commits of trees.
    let diff = commit2_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::TREES},
            ChangesetFileOrdering::Ordered { after: None },
            None,
        )
        .await?;

    let all_changed_dirs_list = [
        "", "a", "a/a", "a/a/a", "d", "i", "j", "q", "r", "r/s", "r/s/t", "r/w",
    ];
    check_diff_paths(&diff, &all_changed_dirs_list);

    // Diff over two commits, filtered by prefix and with a limit.
    let path_restrictions = Some(vec![
        "1".try_into()?,
        "a/a".try_into()?,
        "q".try_into()?,
        "r/s".try_into()?,
    ]);
    let diff = commit2_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            path_restrictions.clone(),
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            Some(3),
        )
        .await?;

    let filtered_changed_files_list = ["1", "a/a/a/a", "a/a/a/b", "q/y", "r/s/t/u"];
    check_diff_paths(&diff, &filtered_changed_files_list[..3]);

    let diff = commit2_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            path_restrictions,
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(filtered_changed_files_list[2].try_into()?),
            },
            Some(3),
        )
        .await?;

    check_diff_paths(&diff, &filtered_changed_files_list[3..]);
    Ok(())
}

#[fbinit::test]
async fn test_ordered_root_diff(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo: BlobRepo = test_repo_factory::build_empty(fb)?;

    // List of file names to test in repo order.  Note in particular that
    // "j.txt" is after "j/k" even though "." is before "/" in lexicographic
    // order, as we sort based on the directory name ("j").
    let file_list = [
        "!", "0", "1", "10", "2", "a/a/a/a", "a/a/a/b", "d/e", "d/g", "i", "j/k", "j.txt", "p",
        "r/s/t/u", "r/v", "r/w/x", "r/y", "z", "é",
    ];

    let mut root = CreateCommitContext::new_root(&ctx, &blobrepo);

    for file in file_list.iter() {
        root = root.add_file(*file, *file);
    }
    let commit = root.commit().await?;

    let mononoke =
        Mononoke::new_test(ctx.clone(), vec![("test".to_string(), blobrepo.clone())]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_ctx = repo
        .changeset(commit)
        .await?
        .ok_or(anyhow!("commit not found"))?;

    let diff = commit_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            None, /* limit */
        )
        .await?;
    check_diff_paths(&diff, &file_list);

    // Test limits and continuation.
    let diff = commit_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            Some(8),
        )
        .await?;
    check_diff_paths(&diff, &file_list[..8]);

    let diff = commit_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(file_list[7].try_into()?),
            },
            Some(8),
        )
        .await?;
    check_diff_paths(&diff, &file_list[8..16]);

    let diff = commit_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(file_list[15].try_into()?),
            },
            Some(8),
        )
        .await?;
    check_diff_paths(&diff, &file_list[16..]);

    let path_restrictions = Some(vec![
        "1".try_into()?,
        "a/a".try_into()?,
        "q".try_into()?,
        "r/s".try_into()?,
    ]);
    let diff = commit_ctx
        .diff_root(
            path_restrictions.clone(),
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            Some(3),
        )
        .await?;

    let filtered_changed_files_list = ["1", "a/a/a/a", "a/a/a/b", "q/y", "r/s/t/u"];
    check_diff_paths(&diff, &filtered_changed_files_list[..3]);

    let diff = commit_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES, ChangesetDiffItem::TREES},
            ChangesetFileOrdering::Ordered { after: None },
            None, /* limit */
        )
        .await?;

    let files_dirs_list = [
        "!", "0", "1", "10", "2", "a", "a/a", "a/a/a", "a/a/a/a", "a/a/a/b", "d", "d/e", "d/g",
        "i", "j", "j/k", "j.txt", "p", "r", "r/s", "r/s/t", "r/s/t/u", "r/v", "r/w", "r/w/x",
        "r/y", "z", "é",
    ];
    check_diff_paths(&diff, &files_dirs_list);

    let diff = commit_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::TREES},
            ChangesetFileOrdering::Ordered { after: None },
            None, /* limit */
        )
        .await?;

    let dirs_list = ["a", "a/a", "a/a/a", "d", "j", "r", "r/s", "r/s/t", "r/w"];
    check_diff_paths(&diff, &dirs_list);

    // a non-root commit2
    let commit2 = CreateCommitContext::new(&ctx, &blobrepo, vec![commit])
        .add_file("second_file", "second_file")
        .delete_file("!")
        .delete_file("0")
        .delete_file("j/k")
        .commit()
        .await?;

    // commit2
    let commit2_ctx = repo
        .changeset(commit2)
        .await?
        .ok_or(anyhow!("commit not found"))?;

    let diff = commit2_ctx
        .diff_root(
            None, /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            None,
        )
        .await?;

    let second_commit_files_list = [
        "1",
        "10",
        "2",
        "a/a/a/a",
        "a/a/a/b",
        "d/e",
        "d/g",
        "i",
        "j.txt",
        "p",
        "r/s/t/u",
        "r/v",
        "r/w/x",
        "r/y",
        "second_file",
        "z",
        "é",
    ];
    check_diff_paths(&diff, &second_commit_files_list);

    Ok(())
}
