/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for `fold_commits` functionality.
//!
//! These tests use the drawdag pattern for clear, visual commit graph definitions.
//! Each test shows the commit graph structure using ASCII art, making it easy to
//! understand the test scenario at a glance.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use chrono::FixedOffset;
use chrono::TimeZone;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use tests_utils::drawdag::extend_from_dag_with_actions;

use crate::CoreContext;
use crate::CreateInfo;
use crate::Repo;
use crate::RepoContext;
use crate::changeset::ChangesetContext;

/// Initialize a repository from a drawdag string.
///
/// The drawdag string defines both the commit graph structure and file changes.
/// Returns a RepoContext and a map of commit names to ChangesetIds.
///
/// # Example DAG syntax
/// ```text
/// A-B-C     # Linear chain
/// # default_files: false
/// # modify: A base.txt "base content"
/// # modify: B file.txt "new content"
/// # delete: C old_file.txt
/// # copy: C new_path.txt "content" B old_path.txt
/// ```
async fn init_repo(
    ctx: &CoreContext,
    dag: &str,
) -> Result<(RepoContext<Repo>, BTreeMap<String, ChangesetId>)> {
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
    let (changesets, _dag) = extend_from_dag_with_actions(ctx, &repo, dag).await?;
    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

/// Extract the copy-from path for a file in a changeset's file_changes.
///
/// Returns:
/// - `Ok(Some(path))` if the file has copy-from info
/// - `Ok(None)` if the file exists but has no copy-from info
/// - `Err` if the file doesn't exist in file_changes or isn't a Change
async fn get_copy_from_path(
    changeset: &ChangesetContext<Repo>,
    path: &str,
) -> Result<Option<NonRootMPath>> {
    let file_changes = changeset.file_changes().await?;
    let path = NonRootMPath::try_from(path)?;
    let file_change = file_changes
        .get(&path)
        .ok_or_else(|| anyhow::anyhow!("path {path} not found in file_changes"))?;
    match file_change {
        FileChange::Change(tracked) => Ok(tracked.copy_from().map(|(p, _cs)| p.clone())),
        _ => Err(anyhow::anyhow!("path {path} is not a Change")),
    }
}

#[mononoke::fbinit_test]
async fn test_fold_commits_different_lines_same_file(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // B modifies line 1, C modifies line 2 of the same file
    // Expected: Folded commit should have both modifications
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "line1\n"
            # modify: B b.txt "line1_modified\nline2_original\nline3\n"
            # modify: C b.txt "line1_modified\nline2_modified\nline3\n"
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await?
        .changeset_ctx;

    // Verify the folded commit has the correct parent
    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], commits["A"]);

    let file_changes = folded.file_changes().await?;
    assert_eq!(file_changes.len(), 1);
    assert!(file_changes.contains_key(&NonRootMPath::try_from("b.txt")?));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_multiple_files_across_directories(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B-C
    // B adds files in dir1/, C adds files in dir2/
    // Expected: Folded commit should have files in both directories
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B dir1/file1.txt "content1\n"
            # modify: B dir1/file2.txt "content2\n"
            # modify: C dir2/file3.txt "content3\n"
            # modify: C dir2/file4.txt "content4\n"
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await?
        .changeset_ctx;

    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], commits["A"]);

    let file_changes = folded.file_changes().await?;
    assert_eq!(file_changes.len(), 4);
    assert!(file_changes.contains_key(&NonRootMPath::try_from("dir1/file1.txt")?));
    assert!(file_changes.contains_key(&NonRootMPath::try_from("dir1/file2.txt")?));
    assert!(file_changes.contains_key(&NonRootMPath::try_from("dir2/file3.txt")?));
    assert!(file_changes.contains_key(&NonRootMPath::try_from("dir2/file4.txt")?));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_single_commit(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // Edge case: folding a single commit (bottom == top)
    // Expected: Should create a new commit identical to the original
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "single commit content\n"
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["B"]), None, None)
        .await?
        .changeset_ctx;

    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], commits["A"]);

    let content = folded
        .path_with_content("b.txt")
        .await?
        .file()
        .await?
        .expect("file should exist")
        .content_concat()
        .await?;
    assert_eq!(content, Bytes::from("single commit content\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_complex_scenario(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C-D
    // B: adds file1, modifies a.txt
    // C: adds file2, deletes file1, modifies a.txt again
    // D: adds file3
    // Expected: Folded should have file2, file3, and final a.txt, but not file1
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C-D
            # default_files: false
            # modify: A a.txt "original\n"
            # modify: B a.txt "first modification\n"
            # modify: B file1.txt "temp file\n"
            # modify: C a.txt "second modification\n"
            # modify: C file2.txt "file2 content\n"
            # delete: C file1.txt
            # modify: D file3.txt "file3 content\n"
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["D"]), None, None)
        .await?
        .changeset_ctx;

    // Verify file1 doesn't exist (was deleted)
    let file1 = folded.path_with_content("file1.txt").await?.file().await?;
    assert!(file1.is_none(), "file1 should be deleted");

    // Verify file2 exists
    let file2 = folded
        .path_with_content("file2.txt")
        .await?
        .file()
        .await?
        .expect("file2 should exist")
        .content_concat()
        .await?;
    assert_eq!(file2, Bytes::from("file2 content\n"));

    // Verify file3 exists
    let file3 = folded
        .path_with_content("file3.txt")
        .await?
        .file()
        .await?
        .expect("file3 should exist")
        .content_concat()
        .await?;
    assert_eq!(file3, Bytes::from("file3 content\n"));

    // Verify a.txt has the final modification
    let a_txt = folded
        .path_with_content("a.txt")
        .await?
        .file()
        .await?
        .expect("a.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(a_txt, Bytes::from("second modification\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_revert_changes(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // B modifies a.txt, C reverts back to original
    // Expected: Fold should fail (empty changes)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "line1\n"
            # modify: B a.txt "line1\nline2\n"
            # modify: C a.txt "line1\n"
        "##,
    )
    .await?;

    let result = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await;

    assert!(
        result.is_err(),
        "Should fail because net changes are empty (revert)"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_file_added_then_deleted(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // B adds a file, C deletes it
    // Expected: Fold should fail (net effect: no change)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B newfile.txt "temporary content\n"
            # delete: C newfile.txt
        "##,
    )
    .await?;

    let result = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await;

    assert!(
        result.is_err(),
        "Should fail because net changes are empty (add then delete)"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_no_op(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // Edge case: both top and additional_changes are None (no-op)
    // Expected: Returns error
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B file1.txt "file1 content\n"
        "##,
    )
    .await?;

    let result = repo.fold_commits(commits["B"], None, None, None).await;

    assert!(result.is_err(), "No-op fold should fail");
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_delete_existing_file(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // B deletes a file that existed in base, C adds a new file
    // Expected: Folded commit should not have the deleted file
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "original content\n"
            # delete: B a.txt
            # modify: C newfile.txt "new content\n"
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await?
        .changeset_ctx;

    // Verify a.txt is deleted
    let file = folded.path_with_content("a.txt").await?.file().await?;
    assert!(file.is_none(), "a.txt should be deleted");

    // Verify newfile.txt exists
    let new_file = folded
        .path_with_content("newfile.txt")
        .await?
        .file()
        .await?
        .expect("newfile.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(new_file, Bytes::from("new content\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_implicit_deletion(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // B adds a file under a directory, C replaces the directory with a file
    // Expected: Files under the directory are implicitly deleted
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B dir1/a.txt "content1\n"
            # modify: C dir1 "content2\n"
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await?
        .changeset_ctx;

    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], commits["A"]);

    // Verify dir1 exists as a file
    let dir1 = folded
        .path_with_content("dir1")
        .await?
        .file()
        .await?
        .expect("dir1 should exist as a file")
        .content_concat()
        .await?;
    assert_eq!(dir1, Bytes::from("content2\n"));

    // Verify dir1/a.txt doesn't exist (implicitly deleted)
    let dir1_a = folded.path_with_content("dir1/a.txt").await?.file().await?;
    assert!(dir1_a.is_none(), "dir1/a.txt should not exist");

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_deleted_implicit_delete(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // A has dir/file and other_file
    // B replaces dir with a file (implicitly deleting dir/file), modifies other_file
    // C deletes the dir file
    // Expected: dir/file should be deleted, not restored
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A dir/file "initial content\n"
            # modify: A other_file "other content\n"
            # modify: B dir "dir is now a file\n"
            # modify: B other_file "modified other content\n"
            # delete: C dir
        "##,
    )
    .await?;

    // Verify dir/file exists in base
    let base = repo.changeset(commits["A"]).await?.expect("base exists");
    let base_dir_file = base.path_with_content("dir/file").await?.file().await?;
    assert!(base_dir_file.is_some(), "dir/file should exist in base");

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await?
        .changeset_ctx;

    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], commits["A"]);

    // Verify dir (the file) doesn't exist
    let dir_file = folded.path_with_content("dir").await?.file().await?;
    assert!(dir_file.is_none(), "dir should not exist as a file");

    // Verify dir/file is deleted (should NOT be restored)
    let dir_subfile = folded.path_with_content("dir/file").await?.file().await?;
    assert!(
        dir_subfile.is_none(),
        "dir/file should be deleted, not restored after dir file was removed"
    );

    // Verify other_file was modified
    let other = folded
        .path_with_content("other_file")
        .await?
        .file()
        .await?
        .expect("other_file should exist")
        .content_concat()
        .await?;
    assert_eq!(other, Bytes::from("modified other content\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_directory_rename_simulation(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // B adds files in old_dir/
    // C "renames" directory by copying files to new_dir/ and deleting old_dir/
    // Expected: Folded commit has files in new_dir/ without copy info (since old_dir didn't exist in A)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B old_dir/file1.txt "content1\n"
            # modify: B old_dir/file2.txt "content2\n"
            # copy: C new_dir/file1.txt "content1\n" B old_dir/file1.txt
            # copy: C new_dir/file2.txt "content2\n" B old_dir/file2.txt
            # delete: C old_dir/file1.txt
            # delete: C old_dir/file2.txt
        "##,
    )
    .await?;

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, None)
        .await?
        .changeset_ctx;

    let file_changes = folded.file_changes().await?;
    assert_eq!(file_changes.len(), 2);

    // Files should exist in new_dir/ without copy_from (since source didn't exist in base)
    let copy_from1 = get_copy_from_path(&folded, "new_dir/file1.txt").await?;
    assert_eq!(
        copy_from1, None,
        "No copy_from since source didn't exist in base"
    );

    let copy_from2 = get_copy_from_path(&folded, "new_dir/file2.txt").await?;
    assert_eq!(
        copy_from2, None,
        "No copy_from since source didn't exist in base"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_move_file_across_directories(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C-D
    // B adds a.txt
    // C moves a.txt to dir1/a.txt
    // D moves dir1/a.txt to dir2/a.txt
    // Folding C..D: should show dir2/a.txt copied from a.txt
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C-D
            # default_files: false
            # modify: A base.txt "base\n"
            # modify: B a.txt "content1\n"
            # copy: C dir1/a.txt "content1\n" B a.txt
            # delete: C a.txt
            # copy: D dir2/a.txt "content1\n" C dir1/a.txt
            # delete: D dir1/a.txt
        "##,
    )
    .await?;

    // Fold C..D (base is B which has a.txt)
    let folded = repo
        .fold_commits(commits["C"], Some(commits["D"]), None, None)
        .await?
        .changeset_ctx;

    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], commits["B"]);

    // Verify dir2/a.txt exists
    let dir2_a = folded
        .path_with_content("dir2/a.txt")
        .await?
        .file()
        .await?
        .expect("dir2/a.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(dir2_a, Bytes::from("content1\n"));

    // Verify intermediate and source files don't exist
    let dir1_a = folded.path_with_content("dir1/a.txt").await?.file().await?;
    assert!(dir1_a.is_none(), "dir1/a.txt should not exist");

    let a = folded.path_with_content("a.txt").await?.file().await?;
    assert!(a.is_none(), "a.txt should not exist");

    // Verify dir2/a.txt has copy_from pointing to a.txt (the original in base commit B)
    let copy_from = get_copy_from_path(&folded, "dir2/a.txt").await?;
    assert_eq!(
        copy_from,
        Some(NonRootMPath::try_from("a.txt")?),
        "dir2/a.txt should be copied from a.txt (chain resolved through dir1/a.txt)"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_chained_renames(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C-D
    // A has original_file
    // B: rename original_file -> file1
    // C: rename file1 -> file2
    // D: rename file2 -> final_file
    // Folding B..D: final_file should have copy_from = original_file
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C-D
            # default_files: false
            # modify: A original_file "content\n"
            # copy: B file1 "content\n" A original_file
            # delete: B original_file
            # copy: C file2 "content\n" B file1
            # delete: C file1
            # copy: D final_file "content\n" C file2
            # delete: D file2
        "##,
    )
    .await?;

    let base = repo.changeset(commits["A"]).await?.expect("base exists");

    let folded = repo
        .fold_commits(commits["B"], Some(commits["D"]), None, None)
        .await?
        .changeset_ctx;

    // Verify the folded commit has the correct parent
    let folded_parents = folded.parents().await?;
    assert_eq!(folded_parents.len(), 1);
    assert_eq!(folded_parents[0], base.id());

    // Verify original_file is deleted
    let original = folded
        .path_with_content("original_file")
        .await?
        .file()
        .await?;
    assert!(original.is_none(), "original_file should be deleted");

    // Verify intermediate files don't exist
    let file1 = folded.path_with_content("file1").await?.file().await?;
    assert!(file1.is_none(), "file1 should not exist");

    let file2 = folded.path_with_content("file2").await?.file().await?;
    assert!(file2.is_none(), "file2 should not exist");

    // Verify final_file exists with correct content
    let final_file = folded
        .path_with_content("final_file")
        .await?
        .file()
        .await?
        .expect("final_file should exist")
        .content_concat()
        .await?;
    assert_eq!(final_file, Bytes::from("content\n"));

    // Verify the copy chain was resolved - final_file should be copied from original_file
    let copy_from = get_copy_from_path(&folded, "final_file").await?;
    assert_eq!(
        copy_from,
        Some(NonRootMPath::try_from("original_file")?),
        "final_file should be copied from original_file (chain resolved)"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_with_custom_author(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // Fold with custom CreateInfo (author, date, message)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B file1.txt "file1 content\n"
            # modify: C file2.txt "file2 content\n"
        "##,
    )
    .await?;

    let custom_author_date = FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2024, 1, 15, 10, 30, 0)
        .unwrap();
    let expected_timestamp = custom_author_date.timestamp();

    let custom_info = CreateInfo {
        author: "Custom Author <custom@example.com>".to_string(),
        author_date: custom_author_date,
        committer: None,
        committer_date: None,
        message: "Custom commit message for folded commits".to_string(),
        extra: BTreeMap::new(),
        git_extra_headers: None,
    };

    let folded = repo
        .fold_commits(commits["B"], Some(commits["C"]), None, Some(custom_info))
        .await?
        .changeset_ctx;

    let commit_info = folded.changeset_info().await?;
    assert_eq!(
        commit_info.author(),
        "Custom Author <custom@example.com>",
        "Author should be the custom author"
    );
    assert_eq!(
        commit_info.author_date().timestamp_secs(),
        expected_timestamp,
        "Author date should match the custom date"
    );
    assert_eq!(
        commit_info.message(),
        "Custom commit message for folded commits",
        "Message should be the custom message"
    );

    // Verify both files exist
    let file1 = folded
        .path_with_content("file1.txt")
        .await?
        .file()
        .await?
        .expect("file1 should exist")
        .content_concat()
        .await?;
    assert_eq!(file1, Bytes::from("file1 content\n"));

    let file2 = folded
        .path_with_content("file2.txt")
        .await?
        .file()
        .await?
        .expect("file2 should exist")
        .content_concat()
        .await?;
    assert_eq!(file2, Bytes::from("file2 content\n"));

    Ok(())
}
