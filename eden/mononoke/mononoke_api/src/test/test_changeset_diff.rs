/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use fbinit::FacebookInit;
use fixtures::ManyFilesDirs;
use fixtures::TestRepoFixture;
use futures::FutureExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use maplit::btreeset;
use maplit::hashmap;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::path::MPath;
use pretty_assertions::assert_eq;
use tests_utils::CreateCommitContext;
use xdiff::CopyInfo;

use crate::ChangesetContext;
use crate::ChangesetDiffItem;
use crate::ChangesetFileOrdering;
use crate::ChangesetPathDiffContext;
use crate::CoreContext;
use crate::HgChangesetId;
use crate::Mononoke;
use crate::RepoContext;
use crate::repo::MononokeRepo;
use crate::repo::Repo;

#[mononoke::fbinit_test]
async fn test_diff_with_moves(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file_to_move", "context1")
        .commit()
        .await?;

    let commit_with_move = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file_with_copy_info("file_moved", "context", (root, "file_to_move"))
        .delete_file("file_to_move")
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_with_move_ctx = repo
        .changeset(commit_with_move)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;
    let diff = commit_with_move_ctx
        .diff_unordered(
            &repo.changeset(root).await?.context("commit not found")?,
            true,  /* include_copies_renames */
            false, /* include_subtree_copies */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
        )
        .await?;

    assert_eq!(diff.len(), 1);
    match diff.first() {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("file_moved")?);
            assert_eq!(
                diff.get_new_content().expect("Should have new").path(),
                &MPath::try_from("file_moved")?
            );
            assert_eq!(
                diff.get_old_content().expect("Should have old").path(),
                &MPath::try_from("file_to_move")?
            );
            assert_eq!(diff.copy_info(), CopyInfo::Move);
        }
        None => {
            panic!("expected a diff");
        }
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_with_multiple_copies(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file_to_copy", "context1")
        .commit()
        .await?;

    let commit_with_copies = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file_with_copy_info("copy_one", "context", (root, "file_to_copy"))
        .add_file_with_copy_info("copy_two", "context", (root, "file_to_copy"))
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_with_copies_ctx = repo
        .changeset(commit_with_copies)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;
    let diff = commit_with_copies_ctx
        .diff_unordered(
            &repo.changeset(root).await?.context("commit not found")?,
            true,  /* include_copies_renames */
            false, /* include_subtree_copies */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
        )
        .await?;

    assert_eq!(diff.len(), 2);
    match diff.first() {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("copy_one")?);
            assert_eq!(
                diff.get_new_content().expect("Should have new").path(),
                &MPath::try_from("copy_one")?
            );
            assert_eq!(
                diff.get_old_content().expect("Should have old").path(),
                &MPath::try_from("file_to_copy")?
            );
            assert_eq!(diff.copy_info(), CopyInfo::Copy);
        }
        None => {
            panic!("expected a diff");
        }
    }
    match diff.get(1) {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("copy_two")?);
            assert_eq!(
                diff.get_new_content().expect("Should have new").path(),
                &MPath::try_from("copy_two")?
            );
            assert_eq!(
                diff.get_old_content().expect("Should have old").path(),
                &MPath::try_from("file_to_copy")?
            );
            assert_eq!(diff.copy_info(), CopyInfo::Copy);
        }
        None => {
            panic!("expected a second diff");
        }
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_with_multiple_moves(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file_to_move", "context1")
        .commit()
        .await?;

    let commit_with_moves = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file_with_copy_info("copy_one", "context", (root, "file_to_move"))
        .add_file_with_copy_info("copy_two", "context", (root, "file_to_move"))
        .add_file_with_copy_info("copy_zzz", "context", (root, "file_to_move"))
        .delete_file("file_to_move")
        .commit()
        .await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo)]).await?;

    let repo = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_with_moves_ctx = repo
        .changeset(commit_with_moves)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;
    let diff = commit_with_moves_ctx
        .diff_unordered(
            &repo.changeset(root).await?.context("commit not found")?,
            true,  /* include_copies_renames */
            false, /* include_subtree_copies */
            None,  /* path_restrictions */
            btreeset! {ChangesetDiffItem::FILES},
        )
        .await?;

    assert_eq!(diff.len(), 3);
    // The first copy of the file becomes a move.
    match diff.first() {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("copy_one")?);
            assert_eq!(
                diff.get_new_content().expect("Should have new").path(),
                &MPath::try_from("copy_one")?
            );
            assert_eq!(
                diff.get_old_content().expect("Should have old").path(),
                &MPath::try_from("file_to_move")?
            );
            assert_eq!(diff.copy_info(), CopyInfo::Move);
        }
        None => {
            panic!("expected a diff");
        }
    }
    match diff.get(1) {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("copy_two")?);
            assert_eq!(
                diff.get_new_content().expect("Should have new").path(),
                &MPath::try_from("copy_two")?
            );
            assert_eq!(
                diff.get_old_content().expect("Should have old").path(),
                &MPath::try_from("file_to_move")?
            );
            assert_eq!(diff.copy_info(), CopyInfo::Copy);
        }
        None => {
            panic!("expected a second diff");
        }
    }
    match diff.get(2) {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("copy_zzz")?);
            assert_eq!(
                diff.get_new_content().expect("Should have new").path(),
                &MPath::try_from("copy_zzz")?
            );
            assert_eq!(
                diff.get_old_content().expect("Should have old").path(),
                &MPath::try_from("file_to_move")?
            );
            assert_eq!(diff.copy_info(), CopyInfo::Copy);
        }
        None => {
            panic!("expected a third diff");
        }
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_with_dirs(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mononoke = Mononoke::new_test(vec![(
        "test".to_string(),
        ManyFilesDirs::get_repo(fb).await,
    )])
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

    let mut diff: Vec<_> = cs
        .diff_unordered(
            &other_cs,
            false,
            false,
            None,
            btreeset! {ChangesetDiffItem::TREES},
        )
        .await?;
    diff.sort_by(|a, b| a.path().cmp(b.path()));
    assert_eq!(diff.len(), 6);
    match diff.first() {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("")?);
            assert_eq!(
                diff.get_new_content().unwrap().path(),
                &MPath::try_from("")?
            );
            assert_eq!(
                diff.get_old_content().unwrap().path(),
                &MPath::try_from("")?
            );
        }
        None => {
            panic!("expected a root dir diff");
        }
    }
    match diff.get(1) {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("dir1")?);
        }
        None => {
            panic!("expected a diff");
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
    let mut diff: Vec<_> = cs
        .diff_unordered(
            &other_cs,
            false,
            false,
            None,
            btreeset! {ChangesetDiffItem::TREES},
        )
        .await?;
    diff.sort_by(|a, b| a.path().cmp(b.path()));
    assert_eq!(diff.len(), 5);
    match diff.first() {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("")?);
            assert_eq!(
                diff.get_new_content().unwrap().path(),
                &MPath::try_from("")?
            );
            assert_eq!(
                diff.get_old_content().unwrap().path(),
                &MPath::try_from("")?
            );
        }
        None => {
            panic!("expected a root dir diff");
        }
    }
    match diff.get(1) {
        Some(diff) => {
            assert_eq!(diff.path(), &MPath::try_from("dir1")?);
            assert!(diff.get_new_content().is_none());
            assert_eq!(
                diff.get_old_content().unwrap().path(),
                &MPath::try_from("dir1")?
            );
        }
        None => {
            panic!("expected a diff");
        }
    }

    Ok(())
}

fn check_diff_paths<R: MononokeRepo>(diff_ctxs: &[ChangesetPathDiffContext<R>], paths: &[&str]) {
    let diff_paths = diff_ctxs
        .iter()
        .map(|diff_ctx| {
            if let (Some(to), Some(from)) = (diff_ctx.get_new_content(), diff_ctx.get_old_content())
            {
                if diff_ctx.copy_info() == CopyInfo::None {
                    assert_eq!(
                        from.path(),
                        to.path(),
                        "paths for changed file do not match"
                    );
                } else {
                    assert_ne!(
                        from.path(),
                        to.path(),
                        "paths for copied or moved file should not match"
                    );
                }
            }
            diff_ctx.path().to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(diff_paths, paths,);
}

#[mononoke::fbinit_test]
async fn test_ordered_diff(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let root = CreateCommitContext::new_root(&ctx, &repo)
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

    let mut commit = CreateCommitContext::new(&ctx, &repo, vec![root]);
    for file in file_list.iter() {
        commit = commit.add_file(*file, *file);
    }
    let commit = commit.commit().await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo.clone())]).await?;

    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_ctx = repo_ctx
        .changeset(commit)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;
    let root_ctx = &repo_ctx
        .changeset(root)
        .await?
        .context("commit not found")?;
    let diff = commit_ctx
        .diff(
            root_ctx,
            false, /* include_copies_renames */
            false, /* include_subtree_copies */
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
            false, /* include_subtree_copies */
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
            false, /* include_subtree_copies */
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
            false, /* include_subtree_copies */
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

    let mut commit2 = CreateCommitContext::new(&ctx, &repo, vec![commit]);
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

    let commit2_ctx = repo_ctx
        .changeset(commit2)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;
    let diff = commit2_ctx
        .diff(
            &commit_ctx,
            true,  /* include_copies_renames */
            false, /* include_subtree_copies */
            None,  /* path_restrictions */
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
            true,  /* include_copies_renames */
            false, /* include_subtree_copies */
            None,  /* path_restrictions */
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
            false, /* include_subtree_copies */
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
            false, /* include_subtree_copies */
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
            false, /* include_subtree_copies */
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

#[mononoke::fbinit_test]
async fn test_ordered_root_diff(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // List of file names to test in repo order.  Note in particular that
    // "j.txt" is after "j/k" even though "." is before "/" in lexicographic
    // order, as we sort based on the directory name ("j").
    let file_list = [
        "!", "0", "1", "10", "2", "a/a/a/a", "a/a/a/b", "d/e", "d/g", "i", "j/k", "j.txt", "p",
        "r/s/t/u", "r/v", "r/w/x", "r/y", "z", "é",
    ];

    let mut root = CreateCommitContext::new_root(&ctx, &repo);

    for file in file_list.iter() {
        root = root.add_file(*file, *file);
    }
    let commit = root.commit().await?;

    let mononoke = Mononoke::new_test(vec![("test".to_string(), repo.clone())]).await?;

    let repo_ctx = mononoke
        .repo(ctx.clone(), "test")
        .await?
        .expect("repo exists")
        .build()
        .await?;
    let commit_ctx = repo_ctx
        .changeset(commit)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;

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
    let commit2 = CreateCommitContext::new(&ctx, &repo, vec![commit])
        .add_file("second_file", "second_file")
        .delete_file("!")
        .delete_file("0")
        .delete_file("j/k")
        .commit()
        .await?;

    // commit2
    let commit2_ctx = repo_ctx
        .changeset(commit2)
        .await?
        .ok_or_else(|| anyhow!("commit not found"))?;

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

async fn build_lfs_enabled_repo(fb: FacebookInit) -> Result<Repo, Error> {
    Ok(test_repo_factory::TestRepoFactory::new(fb)?
        .with_config_override(|cfg| {
            cfg.git_configs.git_lfs_interpret_pointers = true;
        })
        .build()
        .await?)
}

async fn repo_ctx_for_test(ctx: &CoreContext, repo: Repo) -> Result<RepoContext<Repo>, Error> {
    Ok(RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?)
}

async fn changeset_ctx(
    repo_ctx: &RepoContext<Repo>,
    changeset_id: ChangesetId,
    name: &str,
) -> Result<ChangesetContext<Repo>, Error> {
    repo_ctx
        .changeset(changeset_id)
        .await?
        .ok_or_else(|| anyhow!("{name} changeset not found"))
}

async fn build_lfs_flip_pair(
    ctx: &CoreContext,
    repo: &Repo,
    old_lfs: GitLfs,
    new_lfs: GitLfs,
) -> Result<(ChangesetId, ChangesetId), Error> {
    let parent = CreateCommitContext::new_root(ctx, repo)
        .add_file_with_type_and_lfs("binary_file", "shared blob", FileType::Regular, old_lfs)
        .commit()
        .await?;
    let child = CreateCommitContext::new(ctx, repo, vec![parent])
        .add_file_with_type_and_lfs("binary_file", "shared blob", FileType::Regular, new_lfs)
        .commit()
        .await?;

    Ok((parent, child))
}

async fn lfs_change_paths(
    changeset: &ChangesetContext<Repo>,
    other: &ChangesetContext<Repo>,
    path_restrictions: Option<Vec<MPath>>,
) -> Result<Vec<String>, Error> {
    let subtree_copy_sources = manifest::PathTree::default();
    let excluded_paths = std::collections::HashSet::new();
    Ok(changeset
        .get_potential_lfs_changes(
            other,
            &path_restrictions,
            &subtree_copy_sources,
            &excluded_paths,
            None,
            None,
            usize::MAX,
        )
        .await?
        .into_iter()
        .map(|diff| diff.path().to_string())
        .collect())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_detects_lfs_flip(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_lfs_flip_pair(
        &ctx,
        &repo,
        GitLfs::FullContent,
        GitLfs::canonical_pointer(),
    )
    .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    assert_eq!(
        lfs_change_paths(&child_ctx, &parent_ctx, None).await?,
        vec!["binary_file"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_detects_executable_lfs_flip(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "binary_exe",
            "shared blob",
            FileType::Executable,
            GitLfs::FullContent,
        )
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![parent])
        .add_file_with_type_and_lfs(
            "binary_exe",
            "shared blob",
            FileType::Executable,
            GitLfs::canonical_pointer(),
        )
        .commit()
        .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    assert_eq!(
        lfs_change_paths(&child_ctx, &parent_ctx, None).await?,
        vec!["binary_exe"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_detects_immediate_parent_de_lfs_flip(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_lfs_flip_pair(
        &ctx,
        &repo,
        GitLfs::canonical_pointer(),
        GitLfs::FullContent,
    )
    .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    assert_eq!(
        lfs_change_paths(&child_ctx, &parent_ctx, None).await?,
        vec!["binary_file"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_detects_inherited_across_merge(
    fb: FacebookInit,
) -> Result<(), Error> {
    // The path's LFS state was set at root, then inherited unchanged across
    // a merge before the immediate parent. The supplement still detects the
    // renormalize-up flip: the candidate filter sees the LFS-pointer change
    // in the child's bonsai, and the manifest leaves match because LFS
    // pointers and raw blobs share the same effective content_id.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "binary_file",
            "shared blob",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file("anchor", "root")
        .commit()
        .await?;
    let p0 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("p0_marker", "p0")
        .commit()
        .await?;
    let p1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("p1_marker", "p1")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p0, p1])
        .add_file("merge_marker", "m")
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![merge])
        .add_file_with_type_and_lfs(
            "binary_file",
            "shared blob",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .commit()
        .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let merge_ctx = changeset_ctx(&repo_ctx, merge, "merge").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    assert_eq!(
        lfs_change_paths(&child_ctx, &merge_ctx, None).await?,
        vec!["binary_file"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_emits_re_recorded_state(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Documents the producer-invariant assumption: if a commit synthetically
    // re-records a `Change` with the same content+type+LFS as the parent's
    // effective state, the supplement reports it as a flip. Real producers
    // (git import, Sapling commits, the renormalization tool) never emit
    // such no-op entries -- they only record actual changes -- so this case
    // is unreachable in production. If that invariant ever breaks, this
    // test will keep emitting and downstream consumers will see spurious
    // "changed" paths.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "binary_file",
            "shared blob",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .add_file("anchor", "a")
        .commit()
        .await?;
    let b = CreateCommitContext::new(&ctx, &repo, vec![a])
        .add_file("anchor", "b")
        .commit()
        .await?;
    let c = CreateCommitContext::new(&ctx, &repo, vec![b])
        .add_file_with_type_and_lfs(
            "binary_file",
            "shared blob",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .commit()
        .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let b_ctx = changeset_ctx(&repo_ctx, b, "B").await?;
    let c_ctx = changeset_ctx(&repo_ctx, c, "C").await?;

    assert_eq!(
        lfs_change_paths(&c_ctx, &b_ctx, None).await?,
        vec!["binary_file"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_respects_path_restrictions(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "included/lfs_file",
            "shared",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file_with_type_and_lfs(
            "excluded/lfs_file",
            "shared",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![parent])
        .add_file_with_type_and_lfs(
            "included/lfs_file",
            "shared",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .add_file_with_type_and_lfs(
            "excluded/lfs_file",
            "shared",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .commit()
        .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let restrictions = Some(vec![MPath::try_from("included")?]);
    assert_eq!(
        lfs_change_paths(&child_ctx, &parent_ctx, restrictions).await?,
        vec!["included/lfs_file"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_get_potential_lfs_changes_returns_empty_when_repo_lfs_disabled(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let (parent, child) = build_lfs_flip_pair(
        &ctx,
        &repo,
        GitLfs::FullContent,
        GitLfs::canonical_pointer(),
    )
    .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    assert_eq!(
        lfs_change_paths(&child_ctx, &parent_ctx, None).await?,
        Vec::<String>::new(),
    );

    Ok(())
}

/// Build a fixture repo with both manifest-emitted changes (different
/// content at `a_changed` and `c_changed`) and an LFS-only flip
/// (`b_lfs_only`) between two commits.
async fn build_renormalize_with_content_changes(
    ctx: &CoreContext,
    repo: &Repo,
) -> Result<(ChangesetId, ChangesetId), Error> {
    let parent = CreateCommitContext::new_root(ctx, repo)
        .add_file_with_type_and_lfs(
            "a_changed",
            "content A1",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file_with_type_and_lfs(
            "b_lfs_only",
            "shared blob",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file_with_type_and_lfs(
            "c_changed",
            "content C1",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .commit()
        .await?;
    let child = CreateCommitContext::new(ctx, repo, vec![parent])
        .add_file_with_type_and_lfs(
            "a_changed",
            "content A2",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file_with_type_and_lfs(
            "b_lfs_only",
            "shared blob",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .add_file_with_type_and_lfs(
            "c_changed",
            "content C2",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .commit()
        .await?;
    Ok((parent, child))
}

async fn diff_files_only(
    changeset: &ChangesetContext<Repo>,
    other: &ChangesetContext<Repo>,
    ordering: ChangesetFileOrdering,
) -> Result<Vec<ChangesetPathDiffContext<Repo>>, Error> {
    Ok(changeset
        .diff(
            other,
            false,
            false,
            None,
            btreeset! {ChangesetDiffItem::FILES},
            ordering,
            None,
        )
        .await?)
}

fn paths_of<R: MononokeRepo>(diffs: &[ChangesetPathDiffContext<R>]) -> Vec<String> {
    diffs.iter().map(|d| d.path().to_string()).collect()
}

#[mononoke::fbinit_test]
async fn test_diff_ordered_interleaves_supplement_with_manifest(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Ordered diff over a fixture with two content-changed paths
    // (`a_changed`, `c_changed`) and one LFS-only flip in between
    // (`b_lfs_only`) must produce all three paths in sorted order. The
    // supplement path is invisible to the manifest walk and must be merged
    // in by `insert_sorted_results`.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_renormalize_with_content_changes(&ctx, &repo).await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let diff = diff_files_only(
        &child_ctx,
        &parent_ctx,
        ChangesetFileOrdering::Ordered { after: None },
    )
    .await?;
    assert_eq!(
        paths_of(&diff),
        vec!["a_changed", "b_lfs_only", "c_changed"],
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_unordered_includes_supplement(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_renormalize_with_content_changes(&ctx, &repo).await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let diff = diff_files_only(&child_ctx, &parent_ctx, ChangesetFileOrdering::Unordered).await?;
    let paths: std::collections::BTreeSet<String> = paths_of(&diff).into_iter().collect();
    assert_eq!(
        paths,
        ["a_changed", "b_lfs_only", "c_changed"]
            .into_iter()
            .map(String::from)
            .collect(),
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_detects_git_lfs_only_change(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_lfs_flip_pair(
        &ctx,
        &repo,
        GitLfs::FullContent,
        GitLfs::canonical_pointer(),
    )
    .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let unordered =
        diff_files_only(&child_ctx, &parent_ctx, ChangesetFileOrdering::Unordered).await?;
    check_diff_paths(&unordered, &["binary_file"]);

    let ordered = diff_files_only(
        &child_ctx,
        &parent_ctx,
        ChangesetFileOrdering::Ordered { after: None },
    )
    .await?;
    check_diff_paths(&ordered, &["binary_file"]);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_lfs_supplement_skipped_when_jk_disabled(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_lfs_flip_pair(
        &ctx,
        &repo,
        GitLfs::FullContent,
        GitLfs::canonical_pointer(),
    )
    .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let jk_off = JustKnobsInMemory::new(hashmap! {
        "scm/mononoke:changeset_diff_enable_bonsai_only_lfs".to_string()
            => KnobVal::Bool(false),
    });
    let diff = with_just_knobs_async(
        jk_off,
        diff_files_only(&child_ctx, &parent_ctx, ChangesetFileOrdering::Unordered).boxed(),
    )
    .await?;
    check_diff_paths(&diff, &[]);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_lfs_supplement_skipped_when_repo_lfs_disabled(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let (parent, child) = build_lfs_flip_pair(
        &ctx,
        &repo,
        GitLfs::FullContent,
        GitLfs::canonical_pointer(),
    )
    .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let diff = diff_files_only(&child_ctx, &parent_ctx, ChangesetFileOrdering::Unordered).await?;
    let paths: std::collections::BTreeSet<String> =
        diff.iter().map(|d| d.path().to_string()).collect();
    assert!(
        !paths.contains("binary_file"),
        "supplement must not fire when git_lfs_interpret_pointers=false (paths: {paths:?})",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_lfs_supplement_skipped_for_non_first_parent_compare(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;

    // C-vs-A: a "skip-parent" compare where A is not the immediate parent.
    let a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "binary_file",
            "shared blob",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file("anchor", "anchor")
        .commit()
        .await?;
    let b = CreateCommitContext::new(&ctx, &repo, vec![a])
        .add_file("anchor", "anchor_b")
        .commit()
        .await?;
    let c = CreateCommitContext::new(&ctx, &repo, vec![b])
        .add_file_with_type_and_lfs(
            "binary_file",
            "shared blob",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .commit()
        .await?;

    // Merge against non-leftmost parent: M-vs-P1.
    let p0 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "merge_target",
            "shared",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file("p0_marker", "p0")
        .commit()
        .await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type_and_lfs(
            "merge_target",
            "shared",
            FileType::Regular,
            GitLfs::FullContent,
        )
        .add_file("p1_marker", "p1")
        .commit()
        .await?;
    let m = CreateCommitContext::new(&ctx, &repo, vec![p0, p1])
        .add_file_with_type_and_lfs(
            "merge_target",
            "shared",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .commit()
        .await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let a_ctx = changeset_ctx(&repo_ctx, a, "A").await?;
    let c_ctx = changeset_ctx(&repo_ctx, c, "C").await?;
    let p0_ctx = changeset_ctx(&repo_ctx, p0, "P0").await?;
    let p1_ctx = changeset_ctx(&repo_ctx, p1, "P1").await?;
    let m_ctx = changeset_ctx(&repo_ctx, m, "M").await?;

    let diff_c_vs_a = diff_files_only(&c_ctx, &a_ctx, ChangesetFileOrdering::Unordered).await?;
    let paths_c_vs_a: std::collections::BTreeSet<String> =
        diff_c_vs_a.iter().map(|d| d.path().to_string()).collect();
    assert!(
        !paths_c_vs_a.contains("binary_file"),
        "C vs A must not surface binary_file via the supplement (paths: {paths_c_vs_a:?})",
    );

    let diff_m_vs_p1 = diff_files_only(&m_ctx, &p1_ctx, ChangesetFileOrdering::Unordered).await?;
    let paths_m_vs_p1: std::collections::BTreeSet<String> =
        diff_m_vs_p1.iter().map(|d| d.path().to_string()).collect();
    assert!(
        !paths_m_vs_p1.contains("merge_target"),
        "M vs P1 (non-leftmost parent) must not surface merge_target via the supplement (paths: {paths_m_vs_p1:?})",
    );

    let diff_m_vs_p0 = diff_files_only(&m_ctx, &p0_ctx, ChangesetFileOrdering::Unordered).await?;
    let paths_m_vs_p0: std::collections::BTreeSet<String> =
        diff_m_vs_p0.iter().map(|d| d.path().to_string()).collect();
    assert!(
        paths_m_vs_p0.contains("merge_target"),
        "M vs P0 (first parent) must surface merge_target via the supplement (paths: {paths_m_vs_p0:?})",
    );

    Ok(())
}

/// Build a fixture with five paths between parent and child: `a`, `b`, `c`,
/// `d` (optional), `e` are content-changed, and `b` is *also* an LFS flip
/// (so manifest still emits it from content change, supplement filter
/// excludes it). Wait, no: we want a supplement-only path between content
/// changes. Use distinct paths: `a`, `c`, `d`, `e` are content-changed and
/// `b` is supplement-only (LFS flip with same content).
async fn build_renormalize_pair(
    ctx: &CoreContext,
    repo: &Repo,
    include_d: bool,
) -> Result<(ChangesetId, ChangesetId), Error> {
    let mut parent = CreateCommitContext::new_root(ctx, repo)
        .add_file_with_type_and_lfs("a", "a1", FileType::Regular, GitLfs::FullContent)
        .add_file_with_type_and_lfs("b", "shared", FileType::Regular, GitLfs::FullContent)
        .add_file_with_type_and_lfs("c", "c1", FileType::Regular, GitLfs::FullContent)
        .add_file_with_type_and_lfs("e", "e1", FileType::Regular, GitLfs::FullContent);
    if include_d {
        parent =
            parent.add_file_with_type_and_lfs("d", "d1", FileType::Regular, GitLfs::FullContent);
    }
    let parent = parent.commit().await?;

    let mut child = CreateCommitContext::new(ctx, repo, vec![parent])
        .add_file_with_type_and_lfs("a", "a2", FileType::Regular, GitLfs::FullContent)
        .add_file_with_type_and_lfs(
            "b",
            "shared",
            FileType::Regular,
            GitLfs::canonical_pointer(),
        )
        .add_file_with_type_and_lfs("c", "c2", FileType::Regular, GitLfs::FullContent)
        .add_file_with_type_and_lfs("e", "e2", FileType::Regular, GitLfs::FullContent);
    if include_d {
        child = child.add_file_with_type_and_lfs("d", "d2", FileType::Regular, GitLfs::FullContent);
    }
    let child = child.commit().await?;
    Ok((parent, child))
}

#[mononoke::fbinit_test]
async fn test_diff_ordered_renormalize_within_page_sort_order(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_renormalize_pair(&ctx, &repo, false).await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let diff = diff_files_only(
        &child_ctx,
        &parent_ctx,
        ChangesetFileOrdering::Ordered { after: None },
    )
    .await?;
    check_diff_paths(&diff, &["a", "b", "c", "e"]);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_diff_ordered_renormalize_pagination_uses_supplement_cursor(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = build_lfs_enabled_repo(fb).await?;
    let (parent, child) = build_renormalize_pair(&ctx, &repo, true).await?;

    let repo_ctx = repo_ctx_for_test(&ctx, repo).await?;
    let parent_ctx = changeset_ctx(&repo_ctx, parent, "parent").await?;
    let child_ctx = changeset_ctx(&repo_ctx, child, "child").await?;

    let page1 = child_ctx
        .diff(
            &parent_ctx,
            false,
            false,
            None,
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered { after: None },
            Some(2),
        )
        .await?;
    check_diff_paths(&page1, &["a", "b"]);
    let page1_last = page1
        .last()
        .ok_or_else(|| anyhow!("page 1 must not be empty"))?
        .path()
        .clone();

    let page2 = child_ctx
        .diff(
            &parent_ctx,
            false,
            false,
            None,
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(page1_last),
            },
            Some(2),
        )
        .await?;
    check_diff_paths(&page2, &["c", "d"]);
    let page2_last = page2
        .last()
        .ok_or_else(|| anyhow!("page 2 must not be empty"))?
        .path()
        .clone();

    let page3 = child_ctx
        .diff(
            &parent_ctx,
            false,
            false,
            None,
            btreeset! {ChangesetDiffItem::FILES},
            ChangesetFileOrdering::Ordered {
                after: Some(page2_last),
            },
            Some(2),
        )
        .await?;
    check_diff_paths(&page3, &["e"]);

    Ok(())
}
