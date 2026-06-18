/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Utilities for working with manifests and uncommitted changes.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use context::CoreContext;
use format_util::prepend_hg_file_metadata;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use slex::Items;
use slex::Work;
use slex::WorkOptions;
use status::Status;
use storemodel::FileStore;
use storemodel::InsertOpts;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use types::SerializationFormat;
use types::hgid::NULL_ID;
use vfs::VFS;

use crate::metadata::Metadata;

struct InsertFileWork {
    path: RepoPathBuf,
    copy_from: Option<RepoPathBuf>,
}

/// Applies uncommitted changes from status to a manifest.
///
/// This function takes a base manifest and a Status instance (as returned by
/// `WorkingCopy::status()`), reads file content from the VFS, computes file nodes
/// by inserting into the file store, and updates the manifest.
pub fn apply_status(
    ctx: &CoreContext,
    manifest: &mut TreeManifest,
    status: &Status,
    vfs: &VFS,
    file_store: &Arc<dyn FileStore>,
    parent_manifests: &[&TreeManifest],
    mut copymap: HashMap<RepoPathBuf, RepoPathBuf>,
    include_unknown: bool,
) -> Result<()> {
    // Read file size limits from config.
    let soft_limit: ByteCount = ctx.config.must_get("commit", "file-size-limit")?;
    let hard_limit: ByteCount = ctx.config.must_get("devel", "hard-file-size-limit")?;
    let file_size_limit = if soft_limit.value() < hard_limit.value() {
        soft_limit
    } else {
        hard_limit
    };

    // Process removals first.
    // This ensures we handle cases like a file being replaced by a directory.
    for path in status.removed() {
        manifest.remove(path)?;
    }

    // Process added, modified, and optionally unknown files in parallel, then apply to manifest sequentially.
    let work: Vec<_> = status
        .added()
        .chain(status.modified())
        .chain(
            include_unknown
                .then(|| status.unknown())
                .into_iter()
                .flatten(),
        )
        .map(|path| InsertFileWork {
            path: path.clone(),
            copy_from: copymap.remove(path),
        })
        .collect();
    let vfs = vfs.clone();
    let file_store = Arc::clone(file_store);
    let parent_manifests = parent_manifests
        .iter()
        .map(|manifest| (*manifest).clone())
        .collect::<Vec<_>>();
    let results: Vec<_> = Work::try_map(
        WorkOptions::new().inline_items(1),
        Items::ready(work),
        move |work: InsertFileWork| -> Result<_> {
            let parents =
                get_file_parents(&work.path, &parent_manifests, work.copy_from.is_some())?;
            let copy_meta = if file_store.format() == SerializationFormat::Hg
                && let Some(copy_source) = work.copy_from.as_ref()
                && copy_source.as_repo_path() != work.path.as_repo_path()
            {
                get_copy_rev(copy_source, &parent_manifests)?
                    .map(|copy_parent| Key::new(copy_source.clone(), copy_parent))
            } else {
                None
            };
            let metadata = insert_file(
                &work.path,
                &vfs,
                &file_store,
                parents,
                copy_meta,
                file_size_limit,
            )?;
            Ok((work.path, metadata))
        },
    )
    .into_iter()
    .collect::<Result<Vec<_>>>()?;

    for (path, metadata) in results {
        manifest.insert(path, metadata)?;
    }

    Ok(())
}

/// Gets the parent file nodes for a file from the parent manifests.
///
/// Always looks up the destination path in the parent manifests.
/// For copies, the first parent is set to NULL_ID per Hg convention
/// (since copy metadata acts as a "pseudo-parent").
///
/// Returns a Vec of parent file nodes, one for each parent manifest.
/// If the file doesn't exist in a parent manifest, NULL_ID is used.
fn get_file_parents(
    path: &RepoPath,
    parent_manifests: &[TreeManifest],
    is_copy: bool,
) -> Result<Vec<HgId>> {
    let mut parents: Vec<HgId> = parent_manifests
        .iter()
        .map(|m| Ok(m.get_file(path)?.map(|m| m.hgid).unwrap_or_else(|| NULL_ID)))
        .collect::<Result<_>>()?;

    // For copies, set the first parent to NULL_ID per Hg convention.
    // The copy metadata in the file content acts as a pseudo-parent.
    if is_copy && !parents.is_empty() {
        parents[0] = NULL_ID;
    }

    Ok(parents)
}

/// Reads a file from VFS, inserts it into the file store, and returns the resulting FileMetadata.
///
/// If copy_from is provided, embeds copy metadata into the file content for Hg format.
fn insert_file(
    path: &RepoPathBuf,
    vfs: &VFS,
    file_store: &Arc<dyn FileStore>,
    parents: Vec<HgId>,
    copy_meta: Option<Key>,
    file_size_limit: ByteCount,
) -> Result<FileMetadata> {
    let (content, fs_meta) = vfs.read_with_metadata(path)?;

    // Check file size limit before processing.
    let size = content.len() as u64;
    if size >= file_size_limit.value() {
        bail!(
            "{}: size of {} exceeds maximum size of {}!",
            path,
            ByteCount::from(size),
            file_size_limit,
        );
    }

    // Convert std::fs::Metadata to our Metadata type which knows about VFS capabilities.
    let meta: Metadata = fs_meta.into();

    let file_type = if meta.is_symlink(vfs) {
        FileType::Symlink
    } else if meta.is_executable(vfs) {
        FileType::Executable
    } else {
        FileType::Regular
    };

    let content = prepend_hg_file_metadata(content, copy_meta);

    // Insert the file into the store to compute its hgid.
    let opts = InsertOpts {
        parents,
        // We don't remember the trees anywhere yet, so avoid repeatedly inserting the same file to the store.
        read_before_write: true,
        // Use non-permanent storage (i.e. hgcache) if available. We don't want to bloat the repo store.
        permanent: false,
        ..Default::default()
    };
    let hgid = file_store.insert_file(opts, path, content.into())?;

    Ok(FileMetadata::new(hgid, file_type))
}

/// Gets the file node for the copy source from parent manifests.
///
/// Looks in p1 first, then p2 if not found in p1 (for merge scenarios).
fn get_copy_rev(copy_source: &RepoPath, parent_manifests: &[TreeManifest]) -> Result<Option<HgId>> {
    for p in parent_manifests {
        if let Some(meta) = p.get_file(copy_source)? {
            return Ok(Some(meta.hgid));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use format_util;
    use io::IO;
    use manifest::FileType;
    use manifest_tree::TreeManifest;
    use manifest_tree::testutil::TestStore;
    use manifest_tree::testutil::make_tree_manifest;
    use pathmatcher::AlwaysMatcher;
    use status::StatusBuilder;
    use types::testutil::*;

    use super::*;

    /// Creates a CoreContext with default config for testing.
    fn test_context() -> CoreContext {
        test_context_with_size_limit(1024 * 1024)
    }

    /// Creates a CoreContext with a custom file size limit for testing.
    fn test_context_with_size_limit(limit_bytes: u64) -> CoreContext {
        test_context_with_limits(limit_bytes, limit_bytes)
    }

    /// Creates a CoreContext with separate soft and hard file size limits.
    fn test_context_with_limits(soft_limit: u64, hard_limit: u64) -> CoreContext {
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        config.insert(
            "commit.file-size-limit".to_string(),
            format!("{soft_limit}b"),
        );
        config.insert(
            "devel.hard-file-size-limit".to_string(),
            format!("{hard_limit}b"),
        );
        let io = IO::new("".as_bytes(), Vec::new(), Some(Vec::new()));
        CoreContext::new(Arc::new(config), io, Vec::new())
    }

    fn list_files(manifest: &TreeManifest) -> Vec<String> {
        let mut files: Vec<_> = manifest
            .files(AlwaysMatcher::new())
            .map(|f| f.unwrap().path.into_string())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn test_apply_status_with_removals() {
        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(
            tree_store.clone(),
            &[("file1", "10"), ("file2", "20"), ("file3", "30")],
        );
        let parent_manifest = make_tree_manifest(
            tree_store,
            &[("file1", "10"), ("file2", "20"), ("file3", "30")],
        );
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new(tmp.path().to_path_buf()).unwrap();

        let status = StatusBuilder::new()
            .removed(vec![repo_path_buf("file1"), repo_path_buf("file2")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        assert_eq!(list_files(&manifest), vec!["file3"]);
    }

    #[test]
    fn test_apply_status_with_additions() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[("existing", "10")]);
        let parent_manifest = make_tree_manifest(tree_store, &[("existing", "10")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create files in the temp directory
        fs::write(tmp.path().join("new_file"), b"new content").unwrap();
        fs::create_dir_all(tmp.path().join("dir")).unwrap();
        fs::write(tmp.path().join("dir/nested"), b"nested content").unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("new_file"), repo_path_buf("dir/nested")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        assert_eq!(
            list_files(&manifest),
            vec!["dir/nested", "existing", "new_file"]
        );

        // Verify files were inserted with NULL_ID parents (new files not in parent manifest)
        let new_file_meta = manifest.get_file(repo_path("new_file")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"new content", HgId::null_id(), HgId::null_id());
        assert_eq!(new_file_meta.hgid, expected_hgid);

        let nested_meta = manifest.get_file(repo_path("dir/nested")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"nested content", HgId::null_id(), HgId::null_id());
        assert_eq!(nested_meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_with_modifications_hg() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest =
            make_tree_manifest(tree_store.clone(), &[("file1", "10"), ("file2", "20")]);
        let parent_manifest = make_tree_manifest(tree_store, &[("file1", "10"), ("file2", "20")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create files with modified content
        fs::write(tmp.path().join("file1"), b"modified content 1").unwrap();
        fs::write(tmp.path().join("file2"), b"modified content 2").unwrap();

        let status = StatusBuilder::new()
            .modified(vec![repo_path_buf("file1"), repo_path_buf("file2")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        // Verify manifest was updated with new hgids that include parent info
        let file1_meta = manifest.get_file(repo_path("file1")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"modified content 1", &hgid("10"), HgId::null_id());
        assert_eq!(file1_meta.hgid, expected_hgid);

        let file2_meta = manifest.get_file(repo_path("file2")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"modified content 2", &hgid("20"), HgId::null_id());
        assert_eq!(file2_meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_with_modifications_two_parents() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest =
            make_tree_manifest(tree_store.clone(), &[("file1", "10"), ("file2", "20")]);
        let parent1_manifest =
            make_tree_manifest(tree_store.clone(), &[("file1", "10"), ("file2", "10")]);
        let parent2_manifest = make_tree_manifest(tree_store, &[("file1", "20"), ("file2", "20")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create files with modified content
        fs::write(tmp.path().join("file1"), b"modified content 1").unwrap();
        fs::write(tmp.path().join("file2"), b"modified content 2").unwrap();

        let status = StatusBuilder::new()
            .modified(vec![repo_path_buf("file1"), repo_path_buf("file2")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent1_manifest, &parent2_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        // Verify manifest hgids include both parents
        let file1_meta = manifest.get_file(repo_path("file1")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"modified content 1", &hgid("10"), &hgid("20"));
        assert_eq!(file1_meta.hgid, expected_hgid);

        let file2_meta = manifest.get_file(repo_path("file2")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"modified content 2", &hgid("10"), &hgid("20"));
        assert_eq!(file2_meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_with_merge_parents() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[("file", "10")]);
        let parent1_manifest = make_tree_manifest(tree_store.clone(), &[("file", "10")]);
        let parent2_manifest = make_tree_manifest(tree_store, &[("file", "20")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        fs::write(tmp.path().join("file"), b"merged content").unwrap();

        let status = StatusBuilder::new()
            .modified(vec![repo_path_buf("file")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent1_manifest, &parent2_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        // Verify the hash includes both parents
        let file_meta = manifest.get_file(repo_path("file")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"merged content", &hgid("10"), &hgid("20"));
        assert_eq!(file_meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_modified_file_not_in_parent() {
        use std::fs;

        // Test case: file is modified but doesn't exist in parent manifest.
        // This can happen in edge cases. The file should still be inserted with no parents.
        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[("file", "10")]);
        let parent_manifest = make_tree_manifest(tree_store, &[]); // Empty parent
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        fs::write(tmp.path().join("file"), b"content").unwrap();

        let status = StatusBuilder::new()
            .modified(vec![repo_path_buf("file")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        // File should be inserted with NULL_ID parent since it's not in parent manifest
        let file_meta = manifest.get_file(repo_path("file")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"content", HgId::null_id(), HgId::null_id());
        assert_eq!(file_meta.hgid, expected_hgid);
    }

    #[test]
    fn test_get_file_parents_returns_null_for_missing() {
        // Test that get_file_parents returns NULL_ID for files not in parent manifest.
        let tree_store = Arc::new(TestStore::new());
        let parent1_manifest = make_tree_manifest(tree_store.clone(), &[]); // No file in p1
        let parent2_manifest = make_tree_manifest(tree_store, &[("file", "20")]); // File in p2

        let parents = get_file_parents(
            repo_path("file"),
            &[parent1_manifest, parent2_manifest],
            false,
        )
        .unwrap();

        // First parent is NULL_ID, second has the file
        assert_eq!(parents.len(), 2);
        assert!(parents[0].is_null());
        assert_eq!(parents[1], hgid("20"));
    }

    #[test]
    fn test_get_file_parents_with_copy_sets_first_parent_null() {
        // Test that for copies, the first parent is set to NULL_ID.
        let tree_store = Arc::new(TestStore::new());
        let parent_manifest = make_tree_manifest(tree_store, &[("file", "10")]);

        let parents = get_file_parents(repo_path("file"), &[parent_manifest], true).unwrap();

        // For copies, first parent should be NULL_ID
        assert_eq!(parents.len(), 1);
        assert!(parents[0].is_null());
    }

    #[test]
    fn test_apply_status_mixed_operations() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(
            tree_store.clone(),
            &[("keep", "10"), ("modify", "20"), ("remove", "30")],
        );
        let parent_manifest = make_tree_manifest(
            tree_store,
            &[("keep", "10"), ("modify", "20"), ("remove", "30")],
        );
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        fs::write(tmp.path().join("new_file"), b"new content").unwrap();
        fs::write(tmp.path().join("modify"), b"modified content").unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("new_file")])
            .modified(vec![repo_path_buf("modify")])
            .removed(vec![repo_path_buf("remove")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        assert_eq!(list_files(&manifest), vec!["keep", "modify", "new_file"]);

        // Verify added file has NULL_ID parent
        let new_file_meta = manifest.get_file(repo_path("new_file")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"new content", HgId::null_id(), HgId::null_id());
        assert_eq!(new_file_meta.hgid, expected_hgid);

        // Verify modified file has parent
        let modify_meta = manifest.get_file(repo_path("modify")).unwrap().unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"modified content", &hgid("20"), HgId::null_id());
        assert_eq!(modify_meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_empty_status() {
        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[("file", "10")]);
        let parent_manifest = make_tree_manifest(tree_store, &[("file", "10")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new(tmp.path().to_path_buf()).unwrap();

        let status = StatusBuilder::new().build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        assert_eq!(list_files(&manifest), vec!["file"]);
    }

    #[cfg(unix)]
    #[test]
    fn test_apply_status_with_executable_file() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[]);
        let parent_manifest = make_tree_manifest(tree_store, &[]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        let exec_path = tmp.path().join("script.sh");
        fs::write(&exec_path, b"#!/bin/bash\necho hello").unwrap();
        fs::set_permissions(&exec_path, fs::Permissions::from_mode(0o755)).unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("script.sh")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        let meta = manifest.get_file(repo_path("script.sh")).unwrap().unwrap();
        assert_eq!(meta.file_type, FileType::Executable);
    }

    #[cfg(unix)]
    #[test]
    fn test_apply_status_with_symlink() {
        use std::os::unix::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[]);
        let parent_manifest = make_tree_manifest(tree_store, &[]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        let link_path = tmp.path().join("link");
        fs::symlink("target", &link_path).unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("link")])
            .build();

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();

        let meta = manifest.get_file(repo_path("link")).unwrap().unwrap();
        assert_eq!(meta.file_type, FileType::Symlink);

        // Verify the symlink target was stored
        let expected_hgid =
            format_util::hg_sha1_digest(b"target", HgId::null_id(), HgId::null_id());
        assert_eq!(meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_with_copy() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[("original", "10")]);
        let parent_manifest = make_tree_manifest(tree_store, &[("original", "10")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create copied file with new content
        fs::write(tmp.path().join("copied"), b"new content").unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("copied")])
            .build();

        // Set up copymap: copied <- original
        let mut copymap = HashMap::new();
        copymap.insert(repo_path_buf("copied"), repo_path_buf("original"));

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            copymap,
            false,
        )
        .unwrap();

        // Verify copy metadata was embedded by checking the hgid matches
        // content with metadata prepended and NULL_ID parents (copy convention)
        let expected_metadata = format!(
            "\x01\ncopy: original\ncopyrev: {}\n\x01\n",
            hgid("10").to_hex()
        );
        let expected_content = format!("{expected_metadata}new content");
        let expected_hgid = format_util::hg_sha1_digest(
            expected_content.as_bytes(),
            HgId::null_id(),
            HgId::null_id(),
        );
        let meta = manifest.get_file(repo_path("copied")).unwrap().unwrap();
        assert_eq!(meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_copy_source_not_in_parent() {
        use std::fs;

        // Test case: copy source doesn't exist in parent manifest
        // Copy metadata should be skipped, but file should still be added
        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[]);
        let parent_manifest = make_tree_manifest(tree_store, &[]); // Empty parent
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        fs::write(tmp.path().join("copied"), b"content").unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("copied")])
            .build();

        // Set up copymap pointing to non-existent source
        let mut copymap = HashMap::new();
        copymap.insert(repo_path_buf("copied"), repo_path_buf("nonexistent"));

        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            copymap,
            false,
        )
        .unwrap();

        // Verify file was added without copy metadata (source not found in parent).
        // If copy metadata were prepended, the hgid would differ.
        let expected_hgid =
            format_util::hg_sha1_digest(b"content", HgId::null_id(), HgId::null_id());
        let meta = manifest.get_file(repo_path("copied")).unwrap().unwrap();
        assert_eq!(meta.hgid, expected_hgid);
    }

    #[test]
    fn test_apply_status_file_size_limit() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[]);
        let parent_manifest = make_tree_manifest(tree_store, &[]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create a file that exceeds the size limit
        let large_content = vec![b'x'; 1024]; // 1KB of data
        fs::write(tmp.path().join("large_file"), &large_content).unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("large_file")])
            .build();

        // Set size limit to 512 bytes - file is 1024 bytes so should fail
        let result = apply_status(
            &test_context_with_size_limit(512),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("large_file"));
        assert!(err_msg.contains("exceeds maximum size"));
    }

    #[test]
    fn test_apply_status_file_size_limit_allows_smaller_files() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[]);
        let parent_manifest = make_tree_manifest(tree_store, &[]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create a small file that is under the limit
        fs::write(tmp.path().join("small_file"), b"small").unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("small_file")])
            .build();

        // Set size limit to 1024 bytes - file is only 5 bytes so should succeed
        let result = apply_status(
            &test_context_with_size_limit(1024),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_status_soft_limit_cannot_exceed_hard_limit() {
        use std::fs;

        // Test that even if soft limit is set high, the hard limit is enforced.
        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[]);
        let parent_manifest = make_tree_manifest(tree_store, &[]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create a file that is 1KB
        let content = vec![b'x'; 1024];
        fs::write(tmp.path().join("file"), &content).unwrap();

        let status = StatusBuilder::new()
            .added(vec![repo_path_buf("file")])
            .build();

        // Set soft limit to 10KB (high), but hard limit to 512 bytes (low).
        // The effective limit should be 512 bytes (the hard limit).
        let result = apply_status(
            &test_context_with_limits(10 * 1024, 512),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        );

        // File is 1KB which exceeds hard limit of 512 bytes, so should fail
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exceeds maximum size"));
    }

    #[test]
    fn test_apply_status_with_unknown_files() {
        use std::fs;

        let tree_store = Arc::new(TestStore::new());
        let mut manifest = make_tree_manifest(tree_store.clone(), &[("tracked", "10")]);
        let parent_manifest = make_tree_manifest(tree_store, &[("tracked", "10")]);
        let file_store: Arc<dyn FileStore> = Arc::new(TestStore::new());

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        fs::write(tmp.path().join("unknown_file"), b"unknown content").unwrap();

        let status = StatusBuilder::new()
            .unknown(vec![repo_path_buf("unknown_file")])
            .build();

        // With include_unknown=false, unknown files are not added.
        let mut manifest_without = manifest.clone();
        apply_status(
            &test_context(),
            &mut manifest_without,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            false,
        )
        .unwrap();
        assert_eq!(list_files(&manifest_without), vec!["tracked"]);

        // With include_unknown=true, unknown files are added.
        apply_status(
            &test_context(),
            &mut manifest,
            &status,
            &vfs,
            &file_store,
            &[&parent_manifest],
            HashMap::new(),
            true,
        )
        .unwrap();
        assert_eq!(list_files(&manifest), vec!["tracked", "unknown_file"]);

        // Unknown files get NULL_ID parents (not in any parent manifest).
        let unknown_meta = manifest
            .get_file(repo_path("unknown_file"))
            .unwrap()
            .unwrap();
        let expected_hgid =
            format_util::hg_sha1_digest(b"unknown content", HgId::null_id(), HgId::null_id());
        assert_eq!(unknown_meta.hgid, expected_hgid);
    }
}
