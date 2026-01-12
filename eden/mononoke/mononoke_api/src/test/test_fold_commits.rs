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
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::path::MPath;
use phases::PhasesRef;
use tests_utils::drawdag::extend_from_dag_with_actions;

use crate::CoreContext;
use crate::CreateChange;
use crate::CreateChangeFile;
use crate::CreateChangeFileContents;
use crate::CreateChangesetChecks;
use crate::CreateCopyInfo;
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
async fn test_fold_commits_copy_does_not_become_rename(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // A adds file `foo`
    // B copies `foo` to `bar` (foo still exists in B)
    // Expected: After folding, `bar` should be a copy of `foo` and `foo` should still exist
    //           (i.e., the copy should NOT become a rename)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A foo "foo content\n"
            # copy: B bar "foo content\n" A foo
        "##,
    )
    .await?;

    // Verify precondition: foo exists in B
    let b_ctx = repo.changeset(commits["B"]).await?.expect("B exists");
    let foo_in_b = b_ctx.path_with_content("foo").await?;
    assert!(foo_in_b.is_file().await?, "foo should exist as file in B");

    // Verify precondition: bar has copy info from foo in B
    let bar_copy_from = get_copy_from_path(&b_ctx, "bar").await?;
    assert_eq!(
        bar_copy_from,
        Some(NonRootMPath::try_from("foo")?),
        "bar should have copy_from foo in B"
    );

    // Now fold B onto itself (no actual merge, just testing the folding logic)
    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify: foo should still exist after folding (copy should NOT become a rename)
    let foo_after_fold = folded.path_with_content("foo").await?;
    assert!(
        foo_after_fold.is_file().await?,
        "foo should still exist after folding - copy should not become a rename"
    );

    // Verify: bar should still exist after folding
    let bar_after_fold = folded.path_with_content("bar").await?;
    assert!(
        bar_after_fold.is_file().await?,
        "bar should exist after folding"
    );

    // Verify: bar should still have copy info from foo
    let bar_copy_from_after = get_copy_from_path(&folded, "bar").await?;
    assert_eq!(
        bar_copy_from_after,
        Some(NonRootMPath::try_from("foo")?),
        "bar should have copy_from foo in folded commit"
    );

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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["D"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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

    let result = repo
        .fold_commits(
            commits["B"],
            None,
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["C"],
            Some(commits["D"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["D"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
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
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            Some(custom_info),
            CreateChangesetChecks::check(),
        )
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

#[mononoke::fbinit_test]
async fn test_fold_commits_with_additional_changes_add_file(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // additional_changes: add a new file
    // Expected: Folded commit should have both B's changes and the additional file
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

    let additional_changes = BTreeMap::from([(
        MPath::try_from("additional.txt")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("additional content\n"),
                },
                file_type: Some(FileType::Regular),
                git_lfs: None,
            },
            None,
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

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

    let additional = folded
        .path_with_content("additional.txt")
        .await?
        .file()
        .await?
        .expect("additional.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(additional, Bytes::from("additional content\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_with_additional_changes_delete_nonexistent_file(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // additional_changes: try to delete a file that doesn't exist
    // Expected: Should fail because deleted file doesn't exist
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

    let additional_changes =
        BTreeMap::from([(MPath::try_from("nonexistent.txt")?, CreateChange::Deletion)]);

    let result = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(
        result.is_err(),
        "Should fail because deleted file doesn't exist"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_with_additional_changes_delete_file_from_stack(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // additional_changes: delete a file that was added in the stack
    // Expected: Should fail (net effect is empty after deletion cancels addition)
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

    let additional_changes =
        BTreeMap::from([(MPath::try_from("file1.txt")?, CreateChange::Deletion)]);

    let result = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    // This should fail because deleting a file that was just added results in no net change
    assert!(result.is_err(), "Should fail because net effect is empty");

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_with_additional_changes_modify_existing_file(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // additional_changes: modify a file that exists in base
    // Expected: Should succeed with updated content
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

    let additional_changes = BTreeMap::from([(
        MPath::try_from("a.txt")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("modified base\n"),
                },
                file_type: Some(FileType::Regular),
                git_lfs: None,
            },
            None,
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify a.txt has modified content
    let a_txt = folded
        .path_with_content("a.txt")
        .await?
        .file()
        .await?
        .expect("a.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(a_txt, Bytes::from("modified base\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_with_additional_changes_noop_fails(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // additional_changes: add content that matches the file in the stack
    // Expected: Should fail because it's a no-op change
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

    // Try to "add" file1.txt with the same content it already has
    let additional_changes = BTreeMap::from([(
        MPath::try_from("file1.txt")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("file1 content\n"),
                },
                file_type: Some(FileType::Regular),
                git_lfs: None,
            },
            None,
        ),
    )]);

    let result = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(result.is_err(), "Should fail because it's a no-op change");

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_with_additional_changes_only(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // Fold single commit with only additional_changes (top_id = None)
    // Expected: Should succeed with the additional changes applied
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

    let additional_changes = BTreeMap::from([(
        MPath::try_from("additional.txt")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("additional content\n"),
                },
                file_type: Some(FileType::Regular),
                git_lfs: None,
            },
            None,
        ),
    )]);

    // Use None for top_id, relying only on additional_changes
    let folded = repo
        .fold_commits(
            commits["B"],
            None,
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify additional.txt exists
    let additional = folded
        .path_with_content("additional.txt")
        .await?
        .file()
        .await?
        .expect("additional.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(additional, Bytes::from("additional content\n"));

    // file1.txt SHOULD exist because we folded bottom_id (commit B) with additional_changes.
    // When top_id=None, bottom_id becomes the top of a 1-commit stack, so its changes are included.
    let file_changes = folded.file_changes().await?;
    assert_eq!(file_changes.len(), 2);
    assert!(file_changes.contains_key(&NonRootMPath::try_from("additional.txt")?));
    assert!(file_changes.contains_key(&NonRootMPath::try_from("file1.txt")?));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_additional_changes_create_inside_file_without_delete(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // B creates "dir" as a file
    // additional_changes: try to create "dir/subfile" without deleting "dir"
    // Expected: Should fail because "dir" is a file and wasn't deleted
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B dir "dir is a file\n"
        "##,
    )
    .await?;

    let additional_changes = BTreeMap::from([(
        MPath::try_from("dir/subfile.txt")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("subfile content\n"),
                },
                file_type: Some(FileType::Regular),
                git_lfs: None,
            },
            None,
        ),
    )]);

    let result = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(
        result.is_err(),
        "Should fail because creating inside a file path without deleting it"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_additional_changes_create_inside_file_with_delete(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // B creates "dir" as a file
    // additional_changes: delete "dir" and create "dir/subfile"
    // Expected: Should succeed
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B dir "dir is a file\n"
        "##,
    )
    .await?;

    let additional_changes = BTreeMap::from([
        (MPath::try_from("dir")?, CreateChange::Deletion),
        (
            MPath::try_from("dir/subfile.txt")?,
            CreateChange::Tracked(
                CreateChangeFile {
                    contents: CreateChangeFileContents::New {
                        bytes: Bytes::from("subfile content\n"),
                    },
                    file_type: Some(FileType::Regular),
                    git_lfs: None,
                },
                None,
            ),
        ),
    ]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify dir is not a file anymore
    let dir = folded.path_with_content("dir").await?.file().await?;
    assert!(dir.is_none(), "dir should not be a file");

    // Verify dir/subfile.txt exists
    let subfile = folded
        .path_with_content("dir/subfile.txt")
        .await?
        .file()
        .await?
        .expect("dir/subfile.txt should exist")
        .content_concat()
        .await?;
    assert_eq!(subfile, Bytes::from("subfile content\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_public_bottom_not_allowed(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // Mark A and B as public, then try to fold B..C
    // Expected: Should fail because B is public
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "b content\n"
            # modify: C c.txt "c content\n"
        "##,
    )
    .await?;

    // Mark commits A and B as public
    repo.repo()
        .phases()
        .add_reachable_as_public(&ctx, vec![commits["A"], commits["B"]])
        .await?;

    // Try to fold commits B and C - should fail because B is public
    let result = repo
        .fold_commits(
            commits["B"],
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(
        result.is_err(),
        "Folding public commits should fail validation"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_non_linear_stack_fails(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    //         \-D
    // Try to fold C..D (D is not a descendant of C)
    // Expected: Should fail because stack is not linear
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            A-D
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "b content\n"
            # modify: C c.txt "c content\n"
            # modify: D d.txt "d content\n"
        "##,
    )
    .await?;

    // Try to fold C..D - should fail because D is not a descendant of C
    let result = repo
        .fold_commits(
            commits["C"],
            Some(commits["D"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(result.is_err(), "Folding non-linear stack should fail");
    let err = result.err().expect("expected error");
    assert!(
        err.to_string().contains("not linear"),
        "Error should mention non-linear stack: {err}"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_bottom_is_merge_commit_fails(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    //         \-C
    //           \-D (merge B and C)
    //              \-E
    // Try to fold D..E where D is a merge commit
    // Expected: Should fail because bottom commit has multiple parents
    // Note: The implementation may fail with "not linear" or "merged parent" depending
    // on which check runs first
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-D-E
            A-C-D
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "b content\n"
            # modify: C c.txt "c content\n"
            # modify: D d.txt "d content\n"
            # modify: E e.txt "e content\n"
        "##,
    )
    .await?;

    // Try to fold D..E - should fail because D is a merge commit
    let result = repo
        .fold_commits(
            commits["D"],
            Some(commits["E"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    // The exact error may vary - either "not linear" or "merged parent"
    // Both indicate the fold cannot proceed with a merge commit
    assert!(
        result.is_err(),
        "Folding with merge commit at bottom should fail"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_invalid_bottom_id_fails(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // Try to fold with a non-existent bottom commit ID
    // Expected: Should fail because bottom commit doesn't exist
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "b content\n"
            # modify: C c.txt "c content\n"
        "##,
    )
    .await?;

    // Create a fake changeset ID that doesn't exist
    let fake_id = ChangesetId::from_bytes([0u8; 32])?;

    // Try to fold with non-existent bottom - should fail
    let result = repo
        .fold_commits(
            fake_id,
            Some(commits["C"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(
        result.is_err(),
        "Folding with invalid bottom ID should fail"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_invalid_top_id_fails(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B-C
    // Try to fold with a non-existent top commit ID
    // Expected: Should fail because top commit doesn't exist
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B-C
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "b content\n"
            # modify: C c.txt "c content\n"
        "##,
    )
    .await?;

    // Create a fake changeset ID that doesn't exist
    let fake_id = ChangesetId::from_bytes([0u8; 32])?;

    // Try to fold with non-existent top - should fail
    let result = repo
        .fold_commits(
            commits["B"],
            Some(fake_id),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(result.is_err(), "Folding with invalid top ID should fail");

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_root_commit_fails(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // Try to fold A (root commit with no parents)
    // Expected: Should fail because A has no parent
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A a.txt "base\n"
            # modify: B b.txt "b content\n"
        "##,
    )
    .await?;

    // Try to fold the root commit A - should fail because it has no parent
    let result = repo
        .fold_commits(
            commits["A"],
            Some(commits["B"]),
            None,
            None,
            CreateChangesetChecks::check(),
        )
        .await;

    assert!(
        result.is_err(),
        "Folding with root commit at bottom should fail"
    );

    Ok(())
}

/// Helper to get the file type of a file in a changeset
async fn get_file_type(changeset: &ChangesetContext<Repo>, path: &str) -> Result<Option<FileType>> {
    let path_ctx = changeset.path_with_content(path).await?;
    Ok(path_ctx.file_type().await?)
}

#[mononoke::fbinit_test]
async fn test_fold_commits_file_type_inheritance_from_stack(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // B adds file `foo` with type EXEC
    // Additional changes: modify `foo` with file_type: None
    // Expected: `foo` should still have type EXEC (inherited from stack)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A base.txt "base\n"
            # modify: B foo exec "foo content\n"
        "##,
    )
    .await?;

    let b_ctx = repo.changeset(commits["B"]).await?.expect("B exists");
    let foo_type = get_file_type(&b_ctx, "foo").await?;
    assert_eq!(
        foo_type,
        Some(FileType::Executable),
        "foo should be EXEC in B"
    );

    // Additional change: modify foo with file_type: None
    let additional_changes = BTreeMap::from([(
        MPath::try_from("foo")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("modified foo content\n"),
                },
                file_type: None, // Should inherit EXEC from stack
                git_lfs: None,
            },
            None,
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    let folded_foo_type = get_file_type(&folded, "foo").await?;
    assert_eq!(
        folded_foo_type,
        Some(FileType::Executable),
        "foo should inherit EXEC type from stack"
    );

    let content = folded
        .path_with_content("foo")
        .await?
        .file()
        .await?
        .expect("foo should exist")
        .content_concat()
        .await?;
    assert_eq!(content, Bytes::from("modified foo content\n"));

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_file_type_inheritance_through_copy_chain(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B-C
    // A adds file `foo` with type EXEC
    // B renames (copies + deletes) `foo` to `bar`
    // Additional changes: copy `bar` to `baz` with file_type: None
    // Expected: `baz` should have type EXEC (inherited through copy chain: baz <- bar <- foo)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A foo exec "foo content\n"
            # copy: B bar exec "foo content\n" A foo
            # delete: B foo
        "##,
    )
    .await?;

    let a_ctx = repo.changeset(commits["A"]).await?.expect("A exists");
    let foo_type = get_file_type(&a_ctx, "foo").await?;
    assert_eq!(
        foo_type,
        Some(FileType::Executable),
        "foo should be EXEC in A"
    );

    // Verify bar is EXEC in B (inherited from foo)
    let b_ctx = repo.changeset(commits["B"]).await?.expect("B exists");
    let bar_type = get_file_type(&b_ctx, "bar").await?;
    assert_eq!(
        bar_type,
        Some(FileType::Executable),
        "bar should be EXEC in B"
    );

    // Additional change: copy bar to baz with file_type: None
    let additional_changes = BTreeMap::from([(
        MPath::try_from("baz")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("foo content\n"),
                },
                file_type: None, // Should inherit EXEC through copy chain
                git_lfs: None,
            },
            Some(CreateCopyInfo::new(MPath::try_from("bar")?, 0)),
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify baz is EXEC after folding (inherited through copy chain)
    let baz_type = get_file_type(&folded, "baz").await?;
    assert_eq!(
        baz_type,
        Some(FileType::Executable),
        "baz should inherit EXEC type through copy chain (baz <- bar <- foo)"
    );

    let bar_type_after = get_file_type(&folded, "bar").await?;
    assert_eq!(
        bar_type_after,
        Some(FileType::Executable),
        "bar should still be EXEC"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_file_type_inheritance_from_base(fb: FacebookInit) -> Result<(), Error> {
    // Graph: A-B
    // A has file `foo` with type EXEC
    // B modifies `foo` to different content (keeping EXEC)
    // Additional changes: modify `foo` with file_type: None
    // Expected: `foo` should still have type EXEC (inherited from stack state based on A)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A foo exec "foo content\n"
            # modify: B foo exec "modified in B\n"
        "##,
    )
    .await?;

    let a_ctx = repo.changeset(commits["A"]).await?.expect("A exists");
    let foo_type = get_file_type(&a_ctx, "foo").await?;
    assert_eq!(
        foo_type,
        Some(FileType::Executable),
        "foo should be EXEC in A"
    );

    // Additional change: modify foo with file_type: None
    let additional_changes = BTreeMap::from([(
        MPath::try_from("foo")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("additional modification\n"),
                },
                file_type: None, // Should inherit EXEC from working tree (which reflects B's state)
                git_lfs: None,
            },
            None,
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    let folded_foo_type = get_file_type(&folded, "foo").await?;
    assert_eq!(
        folded_foo_type,
        Some(FileType::Executable),
        "foo should inherit EXEC type from stack"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_file_type_explicit_takes_precedence(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // B adds file `foo` with type EXEC
    // Additional changes: modify `foo` with explicit file_type: Regular
    // Expected: `foo` should have type Regular (explicit takes precedence)
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A base.txt "base\n"
            # modify: B foo exec "foo content\n"
        "##,
    )
    .await?;

    // Additional change: modify foo with explicit file_type: Regular
    let additional_changes = BTreeMap::from([(
        MPath::try_from("foo")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("modified foo content\n"),
                },
                file_type: Some(FileType::Regular), // Explicit override
                git_lfs: None,
            },
            None,
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify foo is Regular after folding (explicit type takes precedence)
    let folded_foo_type = get_file_type(&folded, "foo").await?;
    assert_eq!(
        folded_foo_type,
        Some(FileType::Regular),
        "foo should be Regular when explicitly specified"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_fold_commits_file_type_defaults_to_regular_for_new_file(
    fb: FacebookInit,
) -> Result<(), Error> {
    // Graph: A-B
    // Additional changes: add new file with file_type: None
    // Expected: new file should default to Regular
    let ctx = CoreContext::test_mock(fb);
    let (repo, commits) = init_repo(
        &ctx,
        r##"
            A-B
            # default_files: false
            # modify: A base.txt "base\n"
            # modify: B file.txt "file content\n"
        "##,
    )
    .await?;

    // Additional change: add completely new file with file_type: None
    let additional_changes = BTreeMap::from([(
        MPath::try_from("new_file.txt")?,
        CreateChange::Tracked(
            CreateChangeFile {
                contents: CreateChangeFileContents::New {
                    bytes: Bytes::from("new file content\n"),
                },
                file_type: None, // Should default to Regular
                git_lfs: None,
            },
            None,
        ),
    )]);

    let folded = repo
        .fold_commits(
            commits["B"],
            Some(commits["B"]),
            Some(additional_changes),
            None,
            CreateChangesetChecks::check(),
        )
        .await?
        .changeset_ctx;

    // Verify new_file.txt is Regular (default)
    let new_file_type = get_file_type(&folded, "new_file.txt").await?;
    assert_eq!(
        new_file_type,
        Some(FileType::Regular),
        "new file should default to Regular when file_type is None"
    );

    Ok(())
}
