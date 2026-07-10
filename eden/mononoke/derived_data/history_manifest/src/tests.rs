/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use changesets_creation::save_changesets;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::FutureExt;
use futures::TryStreamExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::history_manifest::HistoryManifestDeletedNode;
use mononoke_types::history_manifest::HistoryManifestDirectory;
use mononoke_types::history_manifest::HistoryManifestEntry;
use mononoke_types::history_manifest::HistoryManifestFile;
use mononoke_types::subtree_change::SubtreeChange;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use tests_utils::CreateCommitContext;

use super::*;

#[facet::container]
struct TestRepo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
    RepoIdentity,
);

/// What kind of entry we found at a path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryInfo {
    File {
        linknode: ChangesetId,
        num_parents: usize,
    },
    DeletedNode {
        linknode: ChangesetId,
        num_parents: usize,
    },
    Directory {
        linknode: ChangesetId,
        num_parents: usize,
    },
}

/// Recursively collect all entries in a history manifest directory.
/// Returns a sorted list of (path, entry_info) pairs.
/// Also walks file subentries (for the file-replaces-directory case).
async fn collect_entries(
    ctx: &CoreContext,
    repo: &TestRepo,
    dir: &HistoryManifestDirectory,
    prefix: MPath,
) -> Result<Vec<(String, EntryInfo)>> {
    let blobstore = repo.repo_blobstore();
    let subentries: Vec<_> = dir
        .clone()
        .into_subentries(ctx, blobstore)
        .try_collect()
        .await?;

    let mut result = Vec::new();
    for (name, entry) in subentries {
        let path = prefix.join(&name);
        let mut entry_results = Box::pin(collect_entry_recursive(ctx, repo, entry, path)).await?;
        result.append(&mut entry_results);
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Recursively collect a single entry and its descendants.
async fn collect_entry_recursive(
    ctx: &CoreContext,
    repo: &TestRepo,
    entry: HistoryManifestEntry,
    path: MPath,
) -> Result<Vec<(String, EntryInfo)>> {
    let blobstore = repo.repo_blobstore();
    let path_str = path.to_string();
    let mut result = Vec::new();

    match &entry {
        HistoryManifestEntry::File(id) => {
            let file: HistoryManifestFile = id.load(ctx, blobstore).await?;
            result.push((
                path_str,
                EntryInfo::File {
                    linknode: file.linknode,
                    num_parents: file.parents.len(),
                },
            ));

            // Walk file subentries (file-replaces-directory case).
            let file_subentries: Vec<_> =
                file.into_subentries(ctx, blobstore).try_collect().await?;
            for (sub_name, sub_entry) in file_subentries {
                let sub_path = path.join(&sub_name);
                let mut sub_results =
                    Box::pin(collect_entry_recursive(ctx, repo, sub_entry, sub_path)).await?;
                result.append(&mut sub_results);
            }
        }
        HistoryManifestEntry::DeletedNode(deleted_entry) => {
            let node: HistoryManifestDeletedNode = deleted_entry.load(ctx, blobstore).await?;
            result.push((
                path_str,
                EntryInfo::DeletedNode {
                    linknode: node.linknode,
                    num_parents: node.parents.len(),
                },
            ));
            // Walk deleted node subentries (for directory deletions).
            let node_subentries: Vec<_> = node
                .clone()
                .into_subentries(ctx, blobstore)
                .try_collect()
                .await?;
            for (sub_name, sub_entry) in node_subentries {
                let sub_path = path.join(&sub_name);
                let mut sub_results =
                    Box::pin(collect_entry_recursive(ctx, repo, sub_entry, sub_path)).await?;
                result.append(&mut sub_results);
            }
        }
        HistoryManifestEntry::Directory(id) => {
            let child_dir: HistoryManifestDirectory = id.load(ctx, blobstore).await?;
            result.push((
                path_str,
                EntryInfo::Directory {
                    linknode: child_dir.linknode,
                    num_parents: child_dir.parents.len(),
                },
            ));
            let mut child_entries = Box::pin(collect_entries(ctx, repo, &child_dir, path)).await?;
            result.append(&mut child_entries);
        }
    }

    Ok(result)
}

/// Derive the history manifest for a changeset and return the root directory.
async fn derive_and_load(
    ctx: &CoreContext,
    repo: &TestRepo,
    cs_id: ChangesetId,
) -> Result<HistoryManifestDirectory> {
    let root_id = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(ctx, cs_id, DerivationPriority::LOW)
        .await?;
    let dir = root_id
        .into_history_manifest_directory_id()
        .load(ctx, repo.repo_blobstore())
        .await?;
    Ok(dir)
}

/// Get all file paths (non-deleted) from entries.
fn file_paths(entries: &[(String, EntryInfo)]) -> Vec<&str> {
    entries
        .iter()
        .filter_map(|(path, info)| match info {
            EntryInfo::File { .. } => Some(path.as_str()),
            _ => None,
        })
        .collect()
}

/// Get all deleted file paths from entries.
fn deleted_file_paths(entries: &[(String, EntryInfo)]) -> Vec<&str> {
    entries
        .iter()
        .filter_map(|(path, info)| match info {
            EntryInfo::DeletedNode { .. } => Some(path.as_str()),
            _ => None,
        })
        .collect()
}

/// Find an entry by path.
fn find_entry<'a>(entries: &'a [(String, EntryInfo)], path: &str) -> Option<&'a EntryInfo> {
    entries.iter().find(|(p, _)| p == path).map(|(_, e)| e)
}

/// Root commit with files at various depths and multiple files per directory.
#[mononoke::fbinit_test]
async fn test_root_commit(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("top.txt", "top")
        .add_file("a/mid.txt", "mid")
        .add_file("a/b/c/deep1.txt", "deep1")
        .add_file("a/b/c/deep2.txt", "deep2")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_id).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert_eq!(
        file_paths(&entries),
        vec!["a/b/c/deep1.txt", "a/b/c/deep2.txt", "a/mid.txt", "top.txt"],
    );

    // Root commit — all nodes should have zero parents and linknode cs_id.
    for (_path, info) in &entries {
        match info {
            EntryInfo::File {
                linknode,
                num_parents,
            } => {
                assert_eq!(*num_parents, 0);
                assert_eq!(*linknode, cs_id);
            }
            EntryInfo::Directory {
                linknode,
                num_parents,
            } => {
                assert_eq!(*num_parents, 0);
                assert_eq!(*linknode, cs_id);
            }
            _ => {}
        }
    }

    // Intermediate directories should exist.
    assert!(find_entry(&entries, "a").is_some());
    assert!(find_entry(&entries, "a/b").is_some());
    assert!(find_entry(&entries, "a/b/c").is_some());

    Ok(())
}

/// Modify some files while others remain unchanged.
#[mononoke::fbinit_test]
async fn test_single_parent_modify(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("changed.txt", "v1")
        .add_file("unchanged.txt", "stable")
        .add_file("dir/changed.txt", "v1")
        .add_file("dir/unchanged.txt", "stable")
        .add_file("other/untouched.txt", "stable")
        .commit()
        .await?;

    // Only modify some files — the rest should be carried forward.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("changed.txt", "v2")
        .add_file("dir/changed.txt", "v2")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // All files (changed and unchanged) should be present.
    assert_eq!(
        file_paths(&entries),
        vec![
            "changed.txt",
            "dir/changed.txt",
            "dir/unchanged.txt",
            "other/untouched.txt",
            "unchanged.txt",
        ],
    );

    // Modified files should have linknode cs_b and one parent.
    for path in &["changed.txt", "dir/changed.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
            "unexpected entry for {path}: {entry:?}",
        );
    }

    // Unchanged files should still have linknode cs_a (reused from parent).
    for path in &["unchanged.txt", "dir/unchanged.txt", "other/untouched.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::File { linknode, .. } if *linknode == cs_a),
            "unchanged file {path} should have linknode cs_a: {entry:?}",
        );
    }

    Ok(())
}

/// Delete some files while others remain unchanged.
#[mononoke::fbinit_test]
async fn test_file_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("top.txt", "top")
        .add_file("dir/remove.txt", "remove")
        .add_file("dir/keep.txt", "keep")
        .add_file("other/stable.txt", "stable")
        .commit()
        .await?;

    // Delete some files, leave others untouched.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("top.txt")
        .delete_file("dir/remove.txt")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // Unchanged files should still be live.
    assert_eq!(
        file_paths(&entries),
        vec!["dir/keep.txt", "other/stable.txt"],
    );

    // Deleted files should be marked as deleted.
    assert_eq!(
        deleted_file_paths(&entries),
        vec!["dir/remove.txt", "top.txt"],
    );

    // Deleted files should have linknode cs_b and one parent.
    for path in &["top.txt", "dir/remove.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::DeletedNode { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
            "unexpected entry for {path}: {entry:?}",
        );
    }

    // Unchanged files should retain linknode cs_a.
    for path in &["dir/keep.txt", "other/stable.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::File { linknode, .. } if *linknode == cs_a),
            "unchanged file {path} should have linknode cs_a: {entry:?}",
        );
    }

    Ok(())
}

/// Delete all files in a directory so it becomes a deleted directory.
#[mononoke::fbinit_test]
async fn test_directory_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/a.txt", "a")
        .add_file("dir/b.txt", "b")
        .add_file("dir/sub/c.txt", "c")
        .add_file("other/keep.txt", "keep")
        .commit()
        .await?;

    // Delete every file under "dir/" — the directory should become deleted.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("dir/a.txt")
        .delete_file("dir/b.txt")
        .delete_file("dir/sub/c.txt")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // Only "other/keep.txt" should remain as a live file.
    assert_eq!(file_paths(&entries), vec!["other/keep.txt"]);

    // All entries under "dir/" should be marked as deleted (including dir itself and dir/sub).
    assert_eq!(
        deleted_file_paths(&entries),
        vec!["dir", "dir/a.txt", "dir/b.txt", "dir/sub", "dir/sub/c.txt"],
    );

    // Deleted files should have linknode cs_b and one parent.
    for path in &["dir/a.txt", "dir/b.txt", "dir/sub/c.txt"] {
        let entry = find_entry(&entries, path).expect("deleted file should exist");
        assert!(
            matches!(entry, EntryInfo::DeletedNode { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
            "unexpected entry for {path}: {entry:?}",
        );
    }

    // The "dir" directory should now be a DeletedNode.
    let dir_entry = find_entry(&entries, "dir").expect("dir entry should exist");
    assert!(
        matches!(dir_entry, EntryInfo::DeletedNode { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "dir should be a deleted node with linknode cs_b: {dir_entry:?}",
    );

    // "other" directory should be unchanged with linknode cs_a.
    let other_entry = find_entry(&entries, "other").expect("other entry should exist");
    assert!(
        matches!(other_entry, EntryInfo::Directory { linknode, .. } if *linknode == cs_a),
        "other should have linknode cs_a: {other_entry:?}",
    );

    // The unchanged file should retain linknode cs_a.
    let keep_entry = find_entry(&entries, "other/keep.txt").expect("keep entry should exist");
    assert!(
        matches!(keep_entry, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "other/keep.txt should have linknode cs_a: {keep_entry:?}",
    );

    Ok(())
}

/// Merge commit where the merged file is explicitly resolved.
#[mononoke::fbinit_test]
async fn test_merge_commit(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Diamond: A -> B, A -> C, merge B+C -> D.
    // "stable.txt" is created in A and never modified by any subsequent commit.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("top.txt", "base")
        .add_file("dir/nested.txt", "base")
        .add_file("stable.txt", "unchanged across merge")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("top.txt", "branch b")
        .add_file("dir/nested.txt", "branch b")
        .commit()
        .await?;

    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("top.txt", "branch c")
        .add_file("dir/nested.txt", "branch c")
        .commit()
        .await?;

    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("top.txt", "merged")
        .add_file("dir/nested.txt", "merged")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert_eq!(
        file_paths(&entries),
        vec!["dir/nested.txt", "stable.txt", "top.txt"],
    );

    // Both modified files should have linknode cs_d with 2 parents.
    for path in &["top.txt", "dir/nested.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::File { linknode, num_parents } if *linknode == cs_d && *num_parents == 2),
            "unexpected entry for {path}: {entry:?}",
        );
    }

    // The unmodified file should retain linknode cs_a with no parents
    // (unchanged across the merge).
    let stable = find_entry(&entries, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, num_parents } if *linknode == cs_a && *num_parents == 0),
        "stable.txt should have linknode cs_a and 0 parents: {stable:?}",
    );

    Ok(())
}

/// File replaces a directory — old directory children get deletion entries.
#[mononoke::fbinit_test]
async fn test_file_replaces_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/child1.txt", "c1")
        .add_file("dir/child2.txt", "c2")
        .add_file("dir/sub/deep.txt", "deep")
        .add_file("other.txt", "other")
        .commit()
        .await?;

    // Replace the directory "dir" with a file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("dir", "now a file")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // "dir" should now be a live file, not a directory.
    let dir_entry = find_entry(&entries, "dir").unwrap();
    assert!(
        matches!(dir_entry, EntryInfo::File { linknode, .. } if *linknode == cs_b),
        "dir should be a file with linknode cs_b: {dir_entry:?}",
    );

    // Former directory children should be deleted (including subdirectory entries).
    assert_eq!(
        deleted_file_paths(&entries),
        vec![
            "dir/child1.txt",
            "dir/child2.txt",
            "dir/sub",
            "dir/sub/deep.txt"
        ],
    );

    // Unchanged file should still be live.
    let other = find_entry(&entries, "other.txt").unwrap();
    assert!(
        matches!(other, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "other.txt should have linknode cs_a: {other:?}",
    );

    Ok(())
}

/// File replaces a directory that contains already-deleted children.
/// The already-deleted entries should preserve their original linknode,
/// not get a new one from the replacement commit.
#[mononoke::fbinit_test]
async fn test_file_replaces_directory_with_prior_deletions(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/a.txt", "a")
        .add_file("dir/b.txt", "b")
        .add_file("dir/c.txt", "c")
        .commit()
        .await?;

    // Delete b.txt — it becomes a DeletedFile with linknode cs_b.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("dir/b.txt")
        .commit()
        .await?;

    // Replace the directory with a file. This triggers implicit deletion
    // for all children, including the already-deleted b.txt.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_b])
        .add_file("dir", "now a file")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_c).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // "dir" should be a live file.
    let dir_entry = find_entry(&entries, "dir").unwrap();
    assert!(
        matches!(dir_entry, EntryInfo::File { linknode, .. } if *linknode == cs_c),
        "dir should be a file with linknode cs_c: {dir_entry:?}",
    );

    // a.txt and c.txt were live in the parent — they should have
    // linknode cs_c (deleted by the replacement).
    for path in &["dir/a.txt", "dir/c.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::DeletedNode { linknode, .. } if *linknode == cs_c),
            "{path} should have linknode cs_c (deleted by replacement): {entry:?}",
        );
    }

    // b.txt was already deleted in cs_b — it should preserve linknode
    // cs_b, NOT get a new linknode from cs_c.
    let b_entry = find_entry(&entries, "dir/b.txt").unwrap();
    assert!(
        matches!(b_entry, EntryInfo::DeletedNode { linknode, .. } if *linknode == cs_b),
        "dir/b.txt should preserve original deletion linknode cs_b, not cs_c: {b_entry:?}",
    );

    Ok(())
}

/// Merge where parents disagree on a file (different content on each branch).
#[mononoke::fbinit_test]
async fn test_merge_disagreeing_files(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "base")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "branch b version")
        .commit()
        .await?;

    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "branch c version")
        .commit()
        .await?;

    // Merge without explicitly resolving file.txt — parents disagree.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert_eq!(file_paths(&entries), vec!["file.txt"]);

    // file.txt has no bonsai change — the merge result matches the first
    // parent (cs_b). The first parent's entry is reused.
    let file = find_entry(&entries, "file.txt").unwrap();
    assert!(
        matches!(file, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "file.txt should reuse first parent's entry (linknode cs_b, 1 parent): {file:?}",
    );

    Ok(())
}

/// Merge where one branch deleted a file and the other kept it.
#[mononoke::fbinit_test]
async fn test_merge_deleted_on_one_branch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "content")
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Branch B keeps the file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "modified on b")
        .commit()
        .await?;

    // Branch C deletes the file.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("file.txt")
        .commit()
        .await?;

    // Merge — file.txt is live on B, deleted on C. Parents disagree.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // file.txt should be live — branch B's entry is reused since the
    // merge didn't change the file (no bonsai change, first parent kept).
    assert_eq!(file_paths(&entries), vec!["file.txt", "stable.txt"]);
    assert_eq!(deleted_file_paths(&entries), Vec::<&str>::new());

    // file.txt should reuse branch B's entry (the merge just kept it).
    let file = find_entry(&entries, "file.txt").unwrap();
    assert!(
        matches!(file, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "file.txt should reuse branch B's entry (linknode cs_b, 1 parent): {file:?}",
    );

    // stable.txt unchanged — should retain linknode cs_a.
    let stable = find_entry(&entries, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "stable.txt should have linknode cs_a: {stable:?}",
    );

    Ok(())
}

/// Directory replaces a file — the old file gets a deletion entry.
#[mononoke::fbinit_test]
async fn test_directory_replaces_file(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("path", "I am a file")
        .add_file("other.txt", "stable")
        .commit()
        .await?;

    // Replace the file "path" with a directory by creating files underneath it.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("path/child1.txt", "c1")
        .add_file("path/child2.txt", "c2")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // "path" should now be a live directory, not a file.
    let path_entry = find_entry(&entries, "path").unwrap();
    assert!(
        matches!(path_entry, EntryInfo::Directory { linknode, .. } if *linknode == cs_b),
        "path should be a live directory with linknode cs_b: {path_entry:?}",
    );

    // The new children should be live files with linknode cs_b.
    assert_eq!(
        file_paths(&entries),
        vec!["other.txt", "path/child1.txt", "path/child2.txt"],
    );
    for path in &["path/child1.txt", "path/child2.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 0),
            "unexpected entry for {path}: {entry:?}",
        );
    }

    // Unchanged file should retain linknode cs_a.
    let other = find_entry(&entries, "other.txt").unwrap();
    assert!(
        matches!(other, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "other.txt should have linknode cs_a: {other:?}",
    );

    Ok(())
}

/// File deleted then re-created (undeletion).
#[mononoke::fbinit_test]
async fn test_file_undeletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "original")
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Delete the file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("file.txt")
        .commit()
        .await?;

    // Verify it's deleted at cs_b.
    let root_dir_b = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries_b = collect_entries(&ctx, &repo, &root_dir_b, MPath::ROOT).await?;
    assert_eq!(deleted_file_paths(&entries_b), vec!["file.txt"]);
    assert_eq!(file_paths(&entries_b), vec!["stable.txt"]);

    // Re-create the file with new content.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_b])
        .add_file("file.txt", "resurrected")
        .commit()
        .await?;

    let root_dir_c = derive_and_load(&ctx, &repo, cs_c).await?;
    let entries_c = collect_entries(&ctx, &repo, &root_dir_c, MPath::ROOT).await?;

    // file.txt should be live again with linknode cs_c.
    assert_eq!(file_paths(&entries_c), vec!["file.txt", "stable.txt"]);
    assert_eq!(deleted_file_paths(&entries_c), Vec::<&str>::new());

    let file = find_entry(&entries_c, "file.txt").unwrap();
    assert!(
        matches!(file, EntryInfo::File { linknode, num_parents } if *linknode == cs_c && *num_parents == 1),
        "file.txt should have linknode cs_c and 1 parent: {file:?}",
    );

    // stable.txt unchanged — should retain linknode cs_a.
    let stable = find_entry(&entries_c, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "stable.txt should have linknode cs_a: {stable:?}",
    );

    Ok(())
}

/// Empty commit (no file changes) should produce a manifest identical to its parent.
#[mononoke::fbinit_test]
async fn test_empty_commit(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "content")
        .add_file("dir/nested.txt", "nested")
        .commit()
        .await?;

    // Empty commit — no file changes at all.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // All files should still be present.
    assert_eq!(file_paths(&entries), vec!["dir/nested.txt", "file.txt"]);
    assert_eq!(deleted_file_paths(&entries), Vec::<&str>::new());

    // All files should retain linknode cs_a (nothing changed).
    for path in &["file.txt", "dir/nested.txt"] {
        let entry = find_entry(&entries, path).unwrap();
        assert!(
            matches!(entry, EntryInfo::File { linknode, .. } if *linknode == cs_a),
            "{path} should have linknode cs_a: {entry:?}",
        );
    }

    // Directories should also retain linknode cs_a and not be deleted.
    let dir_entry = find_entry(&entries, "dir").unwrap();
    assert!(
        matches!(dir_entry, EntryInfo::Directory { linknode, .. } if *linknode == cs_a),
        "dir should have linknode cs_a: {dir_entry:?}",
    );

    Ok(())
}

/// Deleting every file in the repo leaves a commit whose root tree is empty.
/// The root must still derive as a `Directory` (matching the unode manifest's
/// behavior of synthesizing an empty root) — never a `DeletedNode`, even
/// though every subentry is deleted.
#[mononoke::fbinit_test]
async fn test_root_all_files_deleted(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("only.txt", "content")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("only.txt")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    assert_eq!(root_dir.linknode, cs_b);

    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;
    assert_eq!(file_paths(&entries), Vec::<&str>::new());
    assert_eq!(deleted_file_paths(&entries), vec!["only.txt"]);

    let entry = find_entry(&entries, "only.txt").unwrap();
    assert!(
        matches!(entry, EntryInfo::DeletedNode { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "only.txt should be a DeletedNode with linknode cs_b: {entry:?}",
    );

    Ok(())
}

/// Directory with mixed live/deleted children stays a Directory.
/// Directory with all deleted children becomes a DeletedNode.
#[mononoke::fbinit_test]
async fn test_directory_deletion_invariant(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/live.txt", "live")
        .add_file("dir/doomed.txt", "doomed")
        .add_file("allgone/x.txt", "x")
        .add_file("allgone/y.txt", "y")
        .commit()
        .await?;

    // Delete one file from each directory, and all files from "allgone".
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("dir/doomed.txt")
        .delete_file("allgone/x.txt")
        .delete_file("allgone/y.txt")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // "dir" has one live and one deleted child — should remain a Directory.
    let dir_entry = find_entry(&entries, "dir").unwrap();
    assert!(
        matches!(dir_entry, EntryInfo::Directory { .. }),
        "dir with mixed live/deleted children should be a Directory: {dir_entry:?}",
    );

    // "allgone" has only deleted children — should be a DeletedNode.
    let allgone_entry = find_entry(&entries, "allgone").unwrap();
    assert!(
        matches!(allgone_entry, EntryInfo::DeletedNode { .. }),
        "allgone with all deleted children should be a DeletedNode: {allgone_entry:?}",
    );

    Ok(())
}

/// Deriving the same commit twice produces identical root IDs.
#[mononoke::fbinit_test]
async fn test_derivation_is_deterministic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a.txt", "a")
        .add_file("dir/b.txt", "b")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("a.txt", "a2")
        .delete_file("dir/b.txt")
        .add_file("dir/c.txt", "c")
        .commit()
        .await?;

    // First derivation.
    let root_id_1 = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_b, DerivationPriority::LOW)
        .await?;

    // Second derivation returns the same result (from cache or recomputation).
    let root_id_2 = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_b, DerivationPriority::LOW)
        .await?;

    assert_eq!(
        root_id_1.into_history_manifest_directory_id(),
        root_id_2.into_history_manifest_directory_id(),
        "Deriving the same commit twice should produce identical root IDs",
    );

    Ok(())
}

/// Merge commit that explicitly deletes a file present in both parents.
#[mononoke::fbinit_test]
async fn test_merge_deletes_file(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "base")
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "branch b")
        .commit()
        .await?;

    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "branch c")
        .commit()
        .await?;

    // Merge explicitly deletes the file.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .delete_file("file.txt")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // file.txt should be deleted with linknode cs_d and 2 parents.
    assert_eq!(deleted_file_paths(&entries), vec!["file.txt"]);
    let file = find_entry(&entries, "file.txt").unwrap();
    assert!(
        matches!(file, EntryInfo::DeletedNode { linknode, num_parents } if *linknode == cs_d && *num_parents == 2),
        "file.txt should be deleted at merge with 2 parents: {file:?}",
    );

    // stable.txt should be unchanged.
    let stable = find_entry(&entries, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "stable.txt should have linknode cs_a: {stable:?}",
    );

    Ok(())
}

/// Merge where both parents independently deleted the same file.
/// The deletion happened in different commits, so the DeletedFile entries
/// have different IDs — this tests merge resolution of deleted entries.
#[mononoke::fbinit_test]
async fn test_merge_both_parents_deleted(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "content")
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Both branches delete the file independently, but make other
    // changes so the bonsai changesets have distinct IDs.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("file.txt")
        .add_file("only_b.txt", "b")
        .commit()
        .await?;

    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("file.txt")
        .add_file("only_c.txt", "c")
        .commit()
        .await?;

    // Merge — both parents deleted file.txt, but their DeletedFile
    // entries have different linknodes (cs_b vs cs_c).
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // file.txt should be deleted.
    assert!(deleted_file_paths(&entries).contains(&"file.txt"));

    // file.txt should have 2 parents — one from each branch's deletion.
    let file = find_entry(&entries, "file.txt").unwrap();
    assert!(
        matches!(file, EntryInfo::DeletedNode { num_parents, .. } if *num_parents == 2),
        "file.txt should have 2 parents from merge of independent deletions: {file:?}",
    );

    Ok(())
}

/// Merge where parents have disjoint files — each parent added files the
/// other doesn't have.
#[mononoke::fbinit_test]
async fn test_merge_disjoint_files(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared.txt", "shared")
        .commit()
        .await?;

    // Branch B adds files.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("only_b.txt", "b stuff")
        .add_file("dir/from_b.txt", "b dir")
        .commit()
        .await?;

    // Branch C adds different files.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("only_c.txt", "c stuff")
        .add_file("dir/from_c.txt", "c dir")
        .commit()
        .await?;

    // Merge picks up all files from both branches.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // All files from both branches should be present.
    assert_eq!(
        file_paths(&entries),
        vec![
            "dir/from_b.txt",
            "dir/from_c.txt",
            "only_b.txt",
            "only_c.txt",
            "shared.txt",
        ],
    );
    assert_eq!(deleted_file_paths(&entries), Vec::<&str>::new());

    // Files unique to one branch should retain their original linknodes.
    let only_b = find_entry(&entries, "only_b.txt").unwrap();
    assert!(
        matches!(only_b, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 0),
        "only_b.txt should have linknode cs_b and 0 parents: {only_b:?}",
    );

    let only_c = find_entry(&entries, "only_c.txt").unwrap();
    assert!(
        matches!(only_c, EntryInfo::File { linknode, num_parents } if *linknode == cs_c && *num_parents == 0),
        "only_c.txt should have linknode cs_c and 0 parents: {only_c:?}",
    );

    // shared.txt unchanged across merge — should retain linknode cs_a.
    let shared = find_entry(&entries, "shared.txt").unwrap();
    assert!(
        matches!(shared, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "shared.txt should have linknode cs_a: {shared:?}",
    );

    Ok(())
}

/// Merge where one parent has a file and the other has a directory at the
/// same path. Parents disagree — one has File("thing"), the other has
/// Directory("thing") — merge_subtrees falls through to directory recursion.
#[mononoke::fbinit_test]
async fn test_merge_file_vs_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Branch B creates "thing" as a file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("thing", "I am a file")
        .commit()
        .await?;

    // Branch C creates "thing" as a directory.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("thing/child.txt", "child content")
        .commit()
        .await?;

    // Merge keeps the directory version by adding thing/child.txt.
    // This resolves the conflict: bonsai has thing/child.txt as a change,
    // which means "thing" is a directory in the merge result.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("thing/child.txt", "child content")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // "thing" should be a directory with 2 parents (File from B,
    // Directory from C).
    let thing_entry = find_entry(&entries, "thing").unwrap();
    assert!(
        matches!(thing_entry, EntryInfo::Directory { num_parents, .. } if *num_parents == 2),
        "thing should be a live directory with 2 parents: {thing_entry:?}",
    );

    // thing/child.txt should be live.
    assert!(file_paths(&entries).contains(&"thing/child.txt"));

    // stable.txt should be unchanged.
    let stable = find_entry(&entries, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "stable.txt should have linknode cs_a: {stable:?}",
    );

    Ok(())
}

/// Merge where one parent added a new directory that the other parent
/// doesn't have at all — asymmetric directory structures.
#[mononoke::fbinit_test]
async fn test_merge_asymmetric_directories(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("root.txt", "root")
        .commit()
        .await?;

    // Branch B adds a deep directory structure.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("new_dir/sub/deep.txt", "deep")
        .add_file("new_dir/top.txt", "top")
        .commit()
        .await?;

    // Branch C makes an unrelated change.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("root.txt", "modified root")
        .commit()
        .await?;

    // Merge picks up both.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("root.txt", "merged root")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // All files should be present.
    assert_eq!(
        file_paths(&entries),
        vec!["new_dir/sub/deep.txt", "new_dir/top.txt", "root.txt"],
    );

    // Files from branch B should retain their linknodes.
    let deep = find_entry(&entries, "new_dir/sub/deep.txt").unwrap();
    assert!(
        matches!(deep, EntryInfo::File { linknode, .. } if *linknode == cs_b),
        "new_dir/sub/deep.txt should have linknode cs_b: {deep:?}",
    );

    // root.txt was resolved in the merge.
    let root = find_entry(&entries, "root.txt").unwrap();
    assert!(
        matches!(root, EntryInfo::File { linknode, num_parents } if *linknode == cs_d && *num_parents == 2),
        "root.txt should have linknode cs_d with 2 parents: {root:?}",
    );

    Ok(())
}

/// Merge where each branch modified a different file in the same directory.
/// Files only changed on one branch should reuse that branch's entry
/// (linknode = branch commit), not create a merge node (linknode = merge).
#[mononoke::fbinit_test]
async fn test_merge_reuses_single_branch_changes(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/x.txt", "base_x")
        .add_file("dir/y.txt", "base_y")
        .add_file("dir/shared.txt", "shared")
        .commit()
        .await?;

    // Branch B modifies x.txt only.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("dir/x.txt", "modified_x")
        .commit()
        .await?;

    // Branch C modifies y.txt only.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("dir/y.txt", "modified_y")
        .commit()
        .await?;

    // Merge picks up both modifications.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("dir/x.txt", "modified_x")
        .add_file("dir/y.txt", "modified_y")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert_eq!(
        file_paths(&entries),
        vec!["dir/shared.txt", "dir/x.txt", "dir/y.txt"],
    );

    // x.txt was only changed on branch B — should reuse B's entry,
    // not create a merge node with linknode cs_d.
    let x = find_entry(&entries, "dir/x.txt").unwrap();
    assert!(
        matches!(x, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "dir/x.txt should reuse branch B's entry (linknode cs_b, 1 parent): {x:?}",
    );

    // y.txt was only changed on branch C — same logic.
    let y = find_entry(&entries, "dir/y.txt").unwrap();
    assert!(
        matches!(y, EntryInfo::File { linknode, num_parents } if *linknode == cs_c && *num_parents == 1),
        "dir/y.txt should reuse branch C's entry (linknode cs_c, 1 parent): {y:?}",
    );

    // shared.txt unchanged across all commits — should retain cs_a.
    let shared = find_entry(&entries, "dir/shared.txt").unwrap();
    assert!(
        matches!(shared, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "dir/shared.txt should have linknode cs_a: {shared:?}",
    );

    Ok(())
}

/// Merge where one parent has a path as a file and the other as a directory,
/// followed by deletion. This is the ambiguous case that motivated unifying
/// DeletedFile and DeletedDirectory into a single DeletedNode type: at the
/// deletion commit, `foo` was simultaneously a file (from one parent) and a
/// directory (from the other), so we can't classify the deletion as either
/// "deleted file" or "deleted directory".
#[mononoke::fbinit_test]
async fn test_ambiguous_file_directory_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Branch B creates "foo" as a file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("foo", "I am a file")
        .commit()
        .await?;

    // Branch C creates "foo" as a directory with a child.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("foo/bar.txt", "child content")
        .commit()
        .await?;

    // Merge keeps the directory version (by including foo/bar.txt).
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("foo/bar.txt", "child content")
        .commit()
        .await?;

    // Now delete foo entirely.
    let cs_e = CreateCommitContext::new(&ctx, &repo, vec![cs_d])
        .delete_file("foo/bar.txt")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_e).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // stable.txt should still be live.
    assert_eq!(file_paths(&entries), vec!["stable.txt"]);

    // "foo" should be a DeletedNode — not a "deleted file" or "deleted
    // directory", but a unified deleted node that covers both cases.
    let foo_entry = find_entry(&entries, "foo").unwrap();
    assert!(
        matches!(foo_entry, EntryInfo::DeletedNode { linknode, .. } if *linknode == cs_e),
        "foo should be a DeletedNode with linknode cs_e: {foo_entry:?}",
    );

    // foo/bar.txt should also be a DeletedNode.
    let bar_entry = find_entry(&entries, "foo/bar.txt").unwrap();
    assert!(
        matches!(bar_entry, EntryInfo::DeletedNode { linknode, .. } if *linknode == cs_e),
        "foo/bar.txt should be a DeletedNode with linknode cs_e: {bar_entry:?}",
    );

    Ok(())
}

/// Octopus merge: three parents with disjoint files.
#[mononoke::fbinit_test]
async fn test_three_parent_merge(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared.txt", "shared")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("from_b.txt", "b")
        .commit()
        .await?;

    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("from_c.txt", "c")
        .commit()
        .await?;

    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("from_d.txt", "d")
        .commit()
        .await?;

    // Three-parent merge.
    let cs_e = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c, cs_d])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_e).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert_eq!(
        file_paths(&entries),
        vec!["from_b.txt", "from_c.txt", "from_d.txt", "shared.txt"],
    );

    // Files unique to each branch should retain their original linknodes.
    let from_b = find_entry(&entries, "from_b.txt").unwrap();
    assert!(
        matches!(from_b, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 0),
        "from_b.txt should have linknode cs_b: {from_b:?}",
    );

    let from_c = find_entry(&entries, "from_c.txt").unwrap();
    assert!(
        matches!(from_c, EntryInfo::File { linknode, num_parents } if *linknode == cs_c && *num_parents == 0),
        "from_c.txt should have linknode cs_c: {from_c:?}",
    );

    let from_d = find_entry(&entries, "from_d.txt").unwrap();
    assert!(
        matches!(from_d, EntryInfo::File { linknode, num_parents } if *linknode == cs_d && *num_parents == 0),
        "from_d.txt should have linknode cs_d: {from_d:?}",
    );

    // shared.txt unchanged — should retain linknode cs_a.
    let shared = find_entry(&entries, "shared.txt").unwrap();
    assert!(
        matches!(shared, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "shared.txt should have linknode cs_a: {shared:?}",
    );

    Ok(())
}

/// Merge where both parents modified the same file and the merge resolves
/// with the content matching one parent exactly.
#[mononoke::fbinit_test]
async fn test_merge_conflict_resolved_to_parent(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "base")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "branch b version")
        .commit()
        .await?;

    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("file.txt", "branch c version")
        .commit()
        .await?;

    // Merge resolves to branch B's content exactly.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("file.txt", "branch b version")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // file.txt should reuse branch B's entry since the merge content
    // matches branch B exactly.
    let file = find_entry(&entries, "file.txt").unwrap();
    assert!(
        matches!(file, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "file.txt should reuse branch B's entry (linknode cs_b, 1 parent): {file:?}",
    );

    Ok(())
}

/// Merge where one parent has a directory at a path and the other has a
/// file. The merge keeps the file version (opposite of
/// test_merge_file_vs_directory which keeps the directory version).
#[mononoke::fbinit_test]
async fn test_merge_keeps_file_over_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Branch B creates "thing" as a directory.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("thing/child.txt", "child")
        .commit()
        .await?;

    // Branch C creates "thing" as a file.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("thing", "I am a file")
        .commit()
        .await?;

    // Merge keeps the file version by explicitly setting thing as a file.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b, cs_c])
        .add_file("thing", "I am a file")
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // "thing" should be a file with linknode cs_d (the merge created a new
    // file node because it needs subentries for the old directory children
    // from branch B) and 2 parents (File from C, Directory from B).
    let thing = find_entry(&entries, "thing").unwrap();
    assert!(
        matches!(thing, EntryInfo::File { linknode, num_parents } if *linknode == cs_d && *num_parents == 2),
        "thing should be a file with linknode cs_d and 2 parents: {thing:?}",
    );

    // The old directory child should appear as a deleted subentry.
    assert!(
        deleted_file_paths(&entries).contains(&"thing/child.txt"),
        "thing/child.txt should be a deleted subentry: {entries:?}",
    );

    // stable.txt unchanged.
    let stable = find_entry(&entries, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "stable.txt should have linknode cs_a: {stable:?}",
    );

    Ok(())
}

/// File replaces directory, then that file is modified. The subentries
/// (old directory children as deletion nodes) should be preserved on the
/// modified file node, not lost.
#[mononoke::fbinit_test]
async fn test_subentries_preserved_on_file_modify(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Create a directory with children.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/child1.txt", "c1")
        .add_file("dir/child2.txt", "c2")
        .commit()
        .await?;

    // Replace directory with a file — children become subentries.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("dir", "now a file")
        .commit()
        .await?;

    // Verify subentries are present after the replacement.
    let root_b = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries_b = collect_entries(&ctx, &repo, &root_b, MPath::ROOT).await?;
    assert!(
        deleted_file_paths(&entries_b).contains(&"dir/child1.txt"),
        "child1.txt should be a deleted subentry after replacement",
    );
    assert!(
        deleted_file_paths(&entries_b).contains(&"dir/child2.txt"),
        "child2.txt should be a deleted subentry after replacement",
    );

    // Modify the file — subentries should be preserved.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_b])
        .add_file("dir", "modified file")
        .commit()
        .await?;

    let root_c = derive_and_load(&ctx, &repo, cs_c).await?;
    let entries_c = collect_entries(&ctx, &repo, &root_c, MPath::ROOT).await?;

    // "dir" should still be a live file.
    let dir_entry = find_entry(&entries_c, "dir").unwrap();
    assert!(
        matches!(dir_entry, EntryInfo::File { linknode, .. } if *linknode == cs_c),
        "dir should be a file with linknode cs_c: {dir_entry:?}",
    );

    // Subentries from the replacement should still be present.
    assert!(
        deleted_file_paths(&entries_c).contains(&"dir/child1.txt"),
        "child1.txt subentry should be preserved after modify: {entries_c:?}",
    );
    assert!(
        deleted_file_paths(&entries_c).contains(&"dir/child2.txt"),
        "child2.txt subentry should be preserved after modify: {entries_c:?}",
    );

    Ok(())
}

/// File replaces directory, then that file is deleted. The subentries
/// should be preserved on the deletion node.
#[mononoke::fbinit_test]
async fn test_subentries_preserved_on_file_delete(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/child.txt", "c")
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Replace directory with a file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("dir", "now a file")
        .commit()
        .await?;

    // Delete the file.
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_b])
        .delete_file("dir")
        .commit()
        .await?;

    let root_c = derive_and_load(&ctx, &repo, cs_c).await?;
    let entries_c = collect_entries(&ctx, &repo, &root_c, MPath::ROOT).await?;

    // "dir" should be a deletion node.
    let dir_entry = find_entry(&entries_c, "dir").unwrap();
    assert!(
        matches!(dir_entry, EntryInfo::DeletedNode { linknode, .. } if *linknode == cs_c),
        "dir should be a DeletedNode with linknode cs_c: {dir_entry:?}",
    );

    // The subentry from the prior replacement should be preserved.
    assert!(
        deleted_file_paths(&entries_c).contains(&"dir/child.txt"),
        "child.txt subentry should be preserved on deletion node: {entries_c:?}",
    );

    Ok(())
}

/// Merge where one parent deleted a file (that had subentries from a prior
/// file-replaces-directory) and the other kept it (also with subentries).
/// The merged node should accumulate subentries from both parents.
/// This exercises the MergeFile path (Case 3, first parent is DeletedNode).
#[mononoke::fbinit_test]
async fn test_subentries_accumulated_on_merge(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Branch B: create dir, replace with file (gets subentry from_b.txt),
    // then delete the file.
    let cs_b1 = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("thing/from_b.txt", "b child")
        .commit()
        .await?;
    let cs_b2 = CreateCommitContext::new(&ctx, &repo, vec![cs_b1])
        .add_file("thing", "file on b")
        .commit()
        .await?;
    let cs_b3 = CreateCommitContext::new(&ctx, &repo, vec![cs_b2])
        .delete_file("thing")
        .commit()
        .await?;

    // Branch C: create dir, replace with file (gets subentry from_c.txt).
    let cs_c1 = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("thing/from_c.txt", "c child")
        .commit()
        .await?;
    let cs_c2 = CreateCommitContext::new(&ctx, &repo, vec![cs_c1])
        .add_file("thing", "file on c")
        .commit()
        .await?;

    // Merge: p1 has DeletedNode(thing), p2 has File(thing).
    // No bonsai change → Case 3: first parent is DeletedNode → MergeFile.
    let cs_d = CreateCommitContext::new(&ctx, &repo, vec![cs_b3, cs_c2])
        .commit()
        .await?;

    let root_d = derive_and_load(&ctx, &repo, cs_d).await?;
    let entries_d = collect_entries(&ctx, &repo, &root_d, MPath::ROOT).await?;

    // "thing" should be a live file (MergeFile uses first File parent's content).
    let thing = find_entry(&entries_d, "thing").unwrap();
    assert!(
        matches!(thing, EntryInfo::File { .. }),
        "thing should be a file: {thing:?}",
    );

    // Subentries from both branches should be present.
    let deleted = deleted_file_paths(&entries_d);
    assert!(
        deleted.contains(&"thing/from_b.txt"),
        "from_b.txt subentry should be present: {entries_d:?}",
    );
    assert!(
        deleted.contains(&"thing/from_c.txt"),
        "from_c.txt subentry should be present: {entries_d:?}",
    );

    Ok(())
}

/// Changing a file's type (Regular → Executable) without changing content.
#[mononoke::fbinit_test]
async fn test_file_type_change(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file_with_type("script.sh", "#!/bin/bash", FileType::Regular)
        .add_file("stable.txt", "stable")
        .commit()
        .await?;

    // Change file type to Executable without changing content.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file_with_type("script.sh", "#!/bin/bash", FileType::Executable)
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert_eq!(file_paths(&entries), vec!["script.sh", "stable.txt"]);

    // script.sh should have a new linknode (cs_b) since the file type
    // changed, even though the content is the same.
    let script = find_entry(&entries, "script.sh").unwrap();
    assert!(
        matches!(script, EntryInfo::File { linknode, num_parents } if *linknode == cs_b && *num_parents == 1),
        "script.sh should have linknode cs_b with 1 parent: {script:?}",
    );

    // stable.txt unchanged.
    let stable = find_entry(&entries, "stable.txt").unwrap();
    assert!(
        matches!(stable, EntryInfo::File { linknode, .. } if *linknode == cs_a),
        "stable.txt should have linknode cs_a: {stable:?}",
    );

    Ok(())
}

/// Commit a bonsai with subtree changes on top of `parents`. Bypasses the
/// subtree-changes justknob gates that would otherwise reject the bonsai.
async fn commit_with_subtree_changes(
    ctx: &CoreContext,
    repo: &TestRepo,
    parents: Vec<ChangesetId>,
    message: &str,
    file_changes: Vec<(&str, Option<(&str, FileType)>)>,
    subtree_changes: Vec<(MPath, SubtreeChange)>,
) -> Result<ChangesetId> {
    let mut ctx_builder = CreateCommitContext::new(ctx, repo, parents).set_message(message);
    for (path, change) in &file_changes {
        ctx_builder = match change {
            Some((content, file_type)) => {
                ctx_builder.add_file_with_type(*path, *content, *file_type)
            }
            None => ctx_builder.delete_file(*path),
        };
    }
    let mut bcs = ctx_builder.create_commit_object().await?;
    bcs.subtree_changes = subtree_changes.into_iter().collect();
    let bcs = bcs.freeze()?;
    let cs_id = bcs.get_changeset_id();
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([(
            "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
            KnobVal::Bool(true),
        )])),
        async { save_changesets(ctx, repo, vec![bcs]).await }.boxed(),
    )
    .await?;
    Ok(cs_id)
}

/// Subtree copy of a directory: destination files should be created fresh
/// (no parents, linknode of the copying commit) while the source and other
/// unrelated paths carry over from the parent.
#[mononoke::fbinit_test]
async fn test_subtree_copy_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Base commit: source directory `a/` with two files, plus an unrelated
    // file `b/file3.txt`.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a/file1.txt", "content1")
        .add_file("a/file2.txt", "content2")
        .add_file("b/file3.txt", "content3")
        .commit()
        .await?;

    // Subtree-copy commit: copy `a/` -> `c/`.
    let cs_b = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_a],
        "subtree copy a -> c",
        vec![],
        vec![(
            MPath::new("c")?,
            SubtreeChange::copy(MPath::new("a")?, cs_a),
        )],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // Source and unrelated files carry over unchanged.
    let src_file1 = find_entry(&entries, "a/file1.txt").unwrap();
    assert!(
        matches!(src_file1, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_a),
        "a/file1.txt should be unchanged from parent: {src_file1:?}",
    );
    let b_file = find_entry(&entries, "b/file3.txt").unwrap();
    assert!(
        matches!(b_file, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_a),
        "b/file3.txt should be unchanged from parent: {b_file:?}",
    );

    // Destination files exist, have cs_b as linknode, and have no parents
    // (the subtree copy destination is rebuilt from scratch).
    let dst_file1 = find_entry(&entries, "c/file1.txt").unwrap();
    assert!(
        matches!(
            dst_file1,
            EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b
        ),
        "c/file1.txt should be a fresh node under cs_b: {dst_file1:?}",
    );
    let dst_file2 = find_entry(&entries, "c/file2.txt").unwrap();
    assert!(
        matches!(
            dst_file2,
            EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b
        ),
        "c/file2.txt should be a fresh node under cs_b: {dst_file2:?}",
    );

    // Destination directory itself should also be fresh (no parents).
    let dst_dir = find_entry(&entries, "c").unwrap();
    assert!(
        matches!(
            dst_dir,
            EntryInfo::Directory { linknode, num_parents: 0 } if *linknode == cs_b
        ),
        "c/ should be a fresh directory under cs_b: {dst_dir:?}",
    );

    // No extra files should appear under c/.
    let unexpected = file_paths(&entries)
        .into_iter()
        .filter(|p| p.starts_with("c/") && *p != "c/file1.txt" && *p != "c/file2.txt")
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "no extra files under c/: {unexpected:?}"
    );

    Ok(())
}

/// Subtree copy of a directory plus an explicit file change inside the
/// destination: the explicit change wins over the synthesized copy, and
/// sibling files in the destination still come from the source.
#[mononoke::fbinit_test]
async fn test_subtree_copy_directory_with_override(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/a.txt", "A-original")
        .add_file("src/b.txt", "B-original")
        .commit()
        .await?;

    // Subtree-copy src -> dst AND override dst/a.txt with new content.
    let cs_b = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_a],
        "subtree copy + override",
        vec![("dst/a.txt", Some(("A-override", FileType::Regular)))],
        vec![(
            MPath::new("dst")?,
            SubtreeChange::copy(MPath::new("src")?, cs_a),
        )],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // dst/b.txt came from the copy: new node, no parents.
    let dst_b = find_entry(&entries, "dst/b.txt").unwrap();
    assert!(
        matches!(dst_b, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "dst/b.txt should be a fresh node: {dst_b:?}",
    );
    // dst/a.txt was explicitly changed in this commit. It's also inside a
    // replacement subtree so its parents should be empty (not inherited
    // from src/a.txt).
    let dst_a = find_entry(&entries, "dst/a.txt").unwrap();
    assert!(
        matches!(dst_a, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "dst/a.txt should be a fresh node with no parents: {dst_a:?}",
    );

    Ok(())
}

/// Subtree copy of a single file: the destination path gets that file.
#[mononoke::fbinit_test]
async fn test_subtree_copy_file(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/only.txt", "only-content")
        .commit()
        .await?;

    let cs_b = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_a],
        "subtree copy single file",
        vec![],
        vec![(
            MPath::new("copied.txt")?,
            SubtreeChange::copy(MPath::new("src/only.txt")?, cs_a),
        )],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    let copied = find_entry(&entries, "copied.txt").unwrap();
    assert!(
        matches!(copied, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "copied.txt should be a fresh node under cs_b: {copied:?}",
    );

    Ok(())
}

/// T1: Nested subtree copies. `a/` → `b/` and `other/` → `b/sub/`. Files
/// from `a/sub/` must NOT leak into `b/sub/`; `b/sub/` should only contain
/// files from `other/`. Exercises the `excluded_paths` filter for nested
/// copy destinations.
#[mononoke::fbinit_test]
async fn test_subtree_copy_nested(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a/top.txt", "a-top")
        .add_file("a/sub/leaked.txt", "a-sub-leaked")
        .add_file("other/y.txt", "other-y")
        .commit()
        .await?;

    let cs_b = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_a],
        "nested subtree copies",
        vec![],
        vec![
            (
                MPath::new("b")?,
                SubtreeChange::copy(MPath::new("a")?, cs_a),
            ),
            (
                MPath::new("b/sub")?,
                SubtreeChange::copy(MPath::new("other")?, cs_a),
            ),
        ],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // b/top.txt comes from the outer copy.
    let b_top = find_entry(&entries, "b/top.txt").unwrap();
    assert!(
        matches!(b_top, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "b/top.txt should be fresh: {b_top:?}",
    );
    // b/sub/y.txt comes from the nested copy; NOT b/sub/leaked.txt.
    let b_sub_y = find_entry(&entries, "b/sub/y.txt").unwrap();
    assert!(
        matches!(b_sub_y, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "b/sub/y.txt should be fresh: {b_sub_y:?}",
    );
    assert!(
        find_entry(&entries, "b/sub/leaked.txt").is_none(),
        "b/sub/leaked.txt must not leak in from the outer copy; entries: {:?}",
        file_paths(&entries),
    );

    Ok(())
}

/// All of the outer copy's source files are excluded by a nested copy
/// that overrides each of them. The outer copy ends up synthesizing no
/// files, but the nested copy still populates the destination, so the
/// commit is valid.
#[mononoke::fbinit_test]
async fn test_subtree_copy_outer_fully_covered_by_nested(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Source `src/` has only `src/leaked.txt`. The outer copy `src` →
    // `dst` would normally synthesize `dst/leaked.txt`, but a nested copy
    // `other` → `dst/leaked.txt` excludes that exact path. So the outer
    // copy synthesizes nothing — only the nested copy's single-file
    // destination remains. The destination is still populated overall,
    // so the preflight emptiness check passes.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/leaked.txt", "leaked")
        .add_file("other.txt", "other")
        .commit()
        .await?;

    let cs_b = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_a],
        "empty-after-exclusion outer copy",
        vec![],
        vec![
            (
                MPath::new("dst")?,
                SubtreeChange::copy(MPath::new("src")?, cs_a),
            ),
            (
                MPath::new("dst/leaked.txt")?,
                SubtreeChange::copy(MPath::new("other.txt")?, cs_a),
            ),
        ],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // The nested copy's destination is present.
    let dst_leaked = find_entry(&entries, "dst/leaked.txt").unwrap();
    assert!(
        matches!(dst_leaked, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "dst/leaked.txt should be fresh: {dst_leaked:?}",
    );
    // The outer destination exists as a fresh directory with only the
    // nested copy's content under it.
    let dst_dir = find_entry(&entries, "dst").unwrap();
    assert!(
        matches!(
            dst_dir,
            EntryInfo::Directory { linknode, num_parents: 0 } if *linknode == cs_b
        ),
        "dst/ should be a fresh directory: {dst_dir:?}",
    );
    let stray = file_paths(&entries)
        .into_iter()
        .filter(|p| p.starts_with("dst/") && *p != "dst/leaked.txt")
        .collect::<Vec<_>>();
    assert!(
        stray.is_empty(),
        "dst/ should contain only the nested copy, got: {stray:?}",
    );

    Ok(())
}

/// T3: Subtree-copy destination replaces an existing directory in the
/// parent. The old content must disappear from the history manifest at
/// that path — the replacement wipes it entirely.
#[mononoke::fbinit_test]
async fn test_subtree_copy_replaces_existing_dir(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Parent has both the source AND an existing `dst/` with different
    // content that's about to be overwritten.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/new.txt", "new-content")
        .add_file("dst/old.txt", "old-content")
        .commit()
        .await?;

    let cs_b = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_a],
        "subtree copy over existing dir",
        vec![],
        vec![(
            MPath::new("dst")?,
            SubtreeChange::copy(MPath::new("src")?, cs_a),
        )],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // dst/new.txt exists from the synthesized copy.
    let dst_new = find_entry(&entries, "dst/new.txt").unwrap();
    assert!(
        matches!(dst_new, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "dst/new.txt should be fresh: {dst_new:?}",
    );
    // dst/old.txt is entirely gone — not as a file, not as a deleted node.
    assert!(
        find_entry(&entries, "dst/old.txt").is_none(),
        "dst/old.txt must be wiped by the subtree copy; entries: {:?}",
        entries.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
    );
    // The dst/ directory itself is fresh.
    let dst_dir = find_entry(&entries, "dst").unwrap();
    assert!(
        matches!(
            dst_dir,
            EntryInfo::Directory { linknode, num_parents: 0 } if *linknode == cs_b
        ),
        "dst/ should be fresh: {dst_dir:?}",
    );

    Ok(())
}

/// T4: Merge commit containing a subtree copy. The subtree copy
/// destination is fresh on top of the merge; regular merge semantics
/// apply elsewhere.
#[mononoke::fbinit_test]
async fn test_subtree_copy_on_merge_commit(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // Common ancestor.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base/x.txt", "x")
        .commit()
        .await?;

    // Two branches, each adding a unique file.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("branch_b.txt", "b")
        .commit()
        .await?;
    let cs_c = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("branch_c.txt", "c")
        .commit()
        .await?;

    // Merge commit with a subtree copy `base` → `copied_base`.
    let cs_merge = commit_with_subtree_changes(
        &ctx,
        &repo,
        vec![cs_b, cs_c],
        "merge + subtree copy",
        vec![],
        vec![(
            MPath::new("copied_base")?,
            SubtreeChange::copy(MPath::new("base")?, cs_a),
        )],
    )
    .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_merge).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    // Subtree copy destination — fresh under cs_merge.
    let copied = find_entry(&entries, "copied_base/x.txt").unwrap();
    assert!(
        matches!(copied, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_merge),
        "copied_base/x.txt should be fresh under cs_merge: {copied:?}",
    );
    // Branch-exclusive files carry over from each parent unchanged.
    let branch_b = find_entry(&entries, "branch_b.txt").unwrap();
    assert!(
        matches!(branch_b, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_b),
        "branch_b.txt should carry over from cs_b: {branch_b:?}",
    );
    let branch_c = find_entry(&entries, "branch_c.txt").unwrap();
    assert!(
        matches!(branch_c, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_c),
        "branch_c.txt should carry over from cs_c: {branch_c:?}",
    );
    // Base path is untouched.
    let base_x = find_entry(&entries, "base/x.txt").unwrap();
    assert!(
        matches!(base_x, EntryInfo::File { linknode, num_parents: 0 } if *linknode == cs_a),
        "base/x.txt should still come from cs_a: {base_x:?}",
    );

    Ok(())
}

/// Subtree copy whose source is a path that no longer exists as a live
/// entry (all descendants were deleted, collapsing the path into a
/// DeletedNode) must error rather than silently producing an empty
/// destination or leaking the parent's content. The Manifest impl for
/// HistoryManifestDirectory filters deleted nodes, so `find_entry`
/// returns `None` for the source path.
#[mononoke::fbinit_test]
async fn test_subtree_copy_empty_source_errors(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    // cs_a creates the source file.
    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/a.txt", "content")
        .add_file("dst/existing.txt", "existing")
        .commit()
        .await?;
    // cs_b deletes everything under src/, so cs_b's history manifest has
    // `src/` as a directory with only a DeletedNode child.
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("src/a.txt")
        .commit()
        .await?;
    // Ensure cs_b's history manifest is derived before we try to copy
    // from it.
    derive_and_load(&ctx, &repo, cs_b).await?;

    // cs_c attempts a subtree copy from cs_b's src/ (which has no live
    // entries) to dst/. dst/ has existing content from cs_a that would
    // otherwise leak through `reused`.
    let cs_c_bcs = {
        let mut bcs = CreateCommitContext::new(&ctx, &repo, vec![cs_b])
            .set_message("copy from empty source")
            .create_commit_object()
            .await?;
        bcs.subtree_changes = vec![(
            MPath::new("dst")?,
            SubtreeChange::copy(MPath::new("src")?, cs_b),
        )]
        .into_iter()
        .collect();
        let bcs = bcs.freeze()?;
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                KnobVal::Bool(true),
            )])),
            async { save_changesets(&ctx, &repo, vec![bcs.clone()]).await }.boxed(),
        )
        .await?;
        bcs
    };
    let cs_c = cs_c_bcs.get_changeset_id();

    let result = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_c, DerivationPriority::LOW)
        .await;
    let err = result.expect_err("derivation should error on empty subtree copy source");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("No subtree copy source"),
        "error should describe the missing source, got: {msg}",
    );

    Ok(())
}

/// Root commit with no file changes derives to an empty Directory.
#[mononoke::fbinit_test]
async fn test_empty_root_commit(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_id = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_id).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert!(entries.is_empty(), "expected no entries, got {entries:?}");
    assert_eq!(root_dir.linknode, cs_id);
    assert!(root_dir.parents.is_empty());

    Ok(())
}

/// Empty commit on top of an empty parent derives to a Directory, not a
/// DeletedNode. An empty inherited subtree is not "all deleted".
#[mononoke::fbinit_test]
async fn test_empty_commit_chain(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo).commit().await?;
    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .commit()
        .await?;

    let root_dir = derive_and_load(&ctx, &repo, cs_b).await?;
    let entries = collect_entries(&ctx, &repo, &root_dir, MPath::ROOT).await?;

    assert!(entries.is_empty(), "expected no entries, got {entries:?}");

    Ok(())
}

/// derive_history_manifest_entry at a non-root prefix returns the same
/// entry as deriving the full tree and extracting at that path.
#[mononoke::fbinit_test]
async fn test_derive_entry_matches_canonical_at_prefix(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir_a/file1.txt", "content1")
        .add_file("dir_a/file2.txt", "content2")
        .add_file("dir_b/file3.txt", "content3")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .add_file("dir_a/file1.txt", "content1_v2")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();
    let derivation_ctx = manager.derivation_context(None);

    let root_a = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_a, DerivationPriority::LOW)
        .await?;
    let root_b = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_b, DerivationPriority::LOW)
        .await?;

    let blobstore = repo.repo_blobstore();
    let bonsai_b = cs_b.load(&ctx, blobstore).await?;

    let prefix = MPath::new("dir_a")?;

    // Look up the stage-level parent entry (dir_a in parent commit A).
    let parent_root_dir: HistoryManifestDirectory = root_a.0.load(&ctx, blobstore).await?;
    let parent_dir_a_entry = parent_root_dir
        .subentries
        .lookup(&ctx, blobstore, b"dir_a")
        .await?;
    let parent_entries: Vec<(ChangesetId, HistoryManifestEntry)> = parent_dir_a_entry
        .into_iter()
        .map(|entry| (cs_a, entry))
        .collect();

    let pipeline_result = crate::derive::derive_history_manifest_entry(
        &ctx,
        &derivation_ctx,
        cs_b,
        &bonsai_b,
        parent_entries,
        prefix.clone(),
        HashMap::new(),
    )
    .await?;

    let canonical_root_dir: HistoryManifestDirectory = root_b.0.load(&ctx, blobstore).await?;
    let canonical_result = canonical_root_dir
        .subentries
        .lookup(&ctx, blobstore, b"dir_a")
        .await?;

    assert_eq!(
        pipeline_result, canonical_result,
        "pipeline entry at prefix should match canonical"
    );
    assert!(
        matches!(pipeline_result, Some(HistoryManifestEntry::Directory(_))),
        "dir_a should be a Directory entry"
    );

    Ok(())
}

/// derive_history_manifest_entry returns None for a path that never existed.
#[mononoke::fbinit_test]
async fn test_derive_entry_nonexistent_path(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir_a/file1.txt", "content")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();
    let derivation_ctx = manager.derivation_context(None);

    let _root_a = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_a, DerivationPriority::LOW)
        .await?;

    let blobstore = repo.repo_blobstore();
    let bonsai_a = cs_a.load(&ctx, blobstore).await?;

    let prefix = MPath::new("nonexistent")?;
    let result = crate::derive::derive_history_manifest_entry(
        &ctx,
        &derivation_ctx,
        cs_a,
        &bonsai_a,
        vec![],
        prefix,
        HashMap::new(),
    )
    .await?;

    assert_eq!(result, None, "nonexistent path should return None");

    Ok(())
}

/// derive_history_manifest_entry returns a DeletedNode for a deleted path.
#[mononoke::fbinit_test]
async fn test_derive_entry_deleted_path(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir_a/file1.txt", "content")
        .add_file("dir_b/file2.txt", "content")
        .commit()
        .await?;

    let cs_b = CreateCommitContext::new(&ctx, &repo, vec![cs_a])
        .delete_file("dir_a/file1.txt")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();
    let derivation_ctx = manager.derivation_context(None);

    let root_a = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_a, DerivationPriority::LOW)
        .await?;
    let root_b = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_b, DerivationPriority::LOW)
        .await?;

    let blobstore = repo.repo_blobstore();
    let bonsai_b = cs_b.load(&ctx, blobstore).await?;

    let prefix = MPath::new("dir_a")?;

    let parent_root_dir: HistoryManifestDirectory = root_a.0.load(&ctx, blobstore).await?;
    let parent_dir_a_entry = parent_root_dir
        .subentries
        .lookup(&ctx, blobstore, b"dir_a")
        .await?;
    let parent_entries: Vec<(ChangesetId, HistoryManifestEntry)> = parent_dir_a_entry
        .into_iter()
        .map(|entry| (cs_a, entry))
        .collect();

    let result = crate::derive::derive_history_manifest_entry(
        &ctx,
        &derivation_ctx,
        cs_b,
        &bonsai_b,
        parent_entries,
        prefix,
        HashMap::new(),
    )
    .await?;

    let canonical_root_dir: HistoryManifestDirectory = root_b.0.load(&ctx, blobstore).await?;
    let canonical_result = canonical_root_dir
        .subentries
        .lookup(&ctx, blobstore, b"dir_a")
        .await?;

    assert_eq!(result, canonical_result);
    assert!(
        matches!(result, Some(HistoryManifestEntry::DeletedNode(_))),
        "dir_a should be a DeletedNode after its only file was deleted"
    );

    Ok(())
}

/// derive_history_manifest_entry at root returns the same directory as
/// derive_history_manifest.
#[mononoke::fbinit_test]
async fn test_derive_entry_at_root_matches_canonical(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let cs_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "content")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();
    let derivation_ctx = manager.derivation_context(None);

    let canonical = repo
        .repo_derived_data()
        .derive::<RootHistoryManifestDirectoryId>(&ctx, cs_a, DerivationPriority::LOW)
        .await?;

    let blobstore = repo.repo_blobstore();
    let bonsai_a = cs_a.load(&ctx, blobstore).await?;

    let result = crate::derive::derive_history_manifest_entry(
        &ctx,
        &derivation_ctx,
        cs_a,
        &bonsai_a,
        vec![],
        MPath::ROOT,
        HashMap::new(),
    )
    .await?;

    assert_eq!(
        result,
        Some(HistoryManifestEntry::Directory(canonical.0)),
        "root prefix should return the same directory as canonical derivation"
    );

    Ok(())
}
