/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

use tempfile::tempdir;

use super::CheckedRelPath;
use super::LiteMetadata;
use super::NoFollowRoot;
use super::OpenFlags;

#[test]
fn checked_rel_path_accepts_relative_paths() -> io::Result<()> {
    assert_eq!(
        CheckedRelPath::try_from(Path::new("a/./b"))?.as_path(),
        Path::new("a/./b")
    );
    Ok(())
}

#[test]
fn checked_rel_path_rejects_escape_paths() {
    assert!(CheckedRelPath::try_from(Path::new("")).is_err());
    assert!(CheckedRelPath::try_from(Path::new("..")).is_err());
    assert!(CheckedRelPath::try_from(Path::new("a/../b")).is_err());
    assert!(CheckedRelPath::try_from(Path::new("/tmp/evil")).is_err());
}

#[cfg(windows)]
#[test]
fn checked_rel_path_rejects_ntfs_ads_paths() {
    assert!(CheckedRelPath::try_from(Path::new("file:Zone.Identifier")).is_err());
    assert!(CheckedRelPath::try_from(Path::new("dir:stream:$DATA")).is_err());
}

#[test]
fn operations_reject_escape_paths() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.write_file(Path::new("../escape"), b"", 0o600).is_err());
    assert!(root.write_file(Path::new("/tmp/evil"), b"", 0o600).is_err());
    assert!(root.remove_file(Path::new("../escape")).is_err());
    assert!(root.remove_dir(Path::new("/tmp/evil")).is_err());
    assert!(root.list_dir(Some(Path::new("../escape"))).is_err());
    assert!(root.list_dir(Some(Path::new("/tmp/evil"))).is_err());
    assert!(!dir.path().join("escape").exists());
    Ok(())
}

#[test]
fn open_root_opens_existing_directory_without_creating() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("sub"))?;
    let root = NoFollowRoot::new(dir.path())?;

    let sub = root.open_root(Path::new("sub"))?;
    sub.write_file(Path::new("file"), b"contents", 0o600)?;

    assert_eq!(fs::read(dir.path().join("sub/file"))?, b"contents".to_vec());
    assert!(root.open_root(Path::new("missing")).is_err());
    assert!(!dir.path().join("missing").exists());
    Ok(())
}

#[test]
fn open_root_rejects_symlink_components() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir_all(dir.path().join("real/child"))?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.open_root(Path::new("link")).is_err());
    assert!(root.open_root(Path::new("a/link/child")).is_err());
    Ok(())
}

#[test]
fn create_dir_creates_leaf_without_creating_parents() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    root.create_dir(Path::new("dir"), Some(0o700))?;

    assert!(dir.path().join("dir").is_dir());
    #[cfg(unix)]
    assert_eq!(
        fs::symlink_metadata(dir.path().join("dir"))?.mode() & 0o777,
        crate::file::apply_umask(0o700)
    );
    assert!(root.create_dir(Path::new("missing/child"), None).is_err());
    assert!(!dir.path().join("missing").exists());
    Ok(())
}

#[test]
fn create_dir_rejects_existing_leaf() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("dir"))?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.create_dir(Path::new("dir"), None).is_err());
    assert!(root.create_dir(Path::new("file"), None).is_err());
    assert!(dir.path().join("dir").is_dir());
    assert_eq!(fs::read(dir.path().join("file"))?, b"contents".to_vec());
    Ok(())
}

#[test]
fn create_dir_rejects_symlink_leaf() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target"))?;
    if !create_dir_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.create_dir(Path::new("link"), None).is_err());
    assert!(fs::symlink_metadata(dir.path().join("link"))?.is_symlink());
    assert!(dir.path().join("target").is_dir());
    Ok(())
}

#[test]
fn create_dir_all_creates_missing_directories() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    root.create_dir_all(Path::new("a/b/c"), Some(0o700))?;
    root.create_dir_all(Path::new("a/b/c"), Some(0o755))?;

    assert!(dir.path().join("a/b/c").is_dir());
    #[cfg(unix)]
    assert_eq!(
        fs::symlink_metadata(dir.path().join("a"))?.mode() & 0o777,
        crate::file::apply_umask(0o700)
    );
    Ok(())
}

#[test]
fn create_dir_all_rejects_existing_file_leaf() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.create_dir_all(Path::new("file"), None).is_err());
    assert_eq!(fs::read(dir.path().join("file"))?, b"contents".to_vec());
    Ok(())
}

#[test]
fn create_dir_all_rejects_leaf_symlink_to_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target"))?;
    if !create_dir_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.create_dir_all(Path::new("link/child"), None).is_err());
    assert!(fs::symlink_metadata(dir.path().join("link"))?.is_symlink());
    assert!(!dir.path().join("target/child").exists());
    Ok(())
}

#[test]
fn create_dir_all_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.create_dir_all(Path::new("a/link/child"), None)
            .is_err()
    );
    assert!(!dir.path().join("real/child").exists());
    assert!(fs::symlink_metadata(dir.path().join("a/link"))?.is_symlink());
    Ok(())
}

#[test]
fn list_dir_lists_root_entries() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    fs::create_dir(dir.path().join("sub"))?;
    let root = NoFollowRoot::new(dir.path())?;

    assert_eq!(
        sorted_names(root.list_dir(None::<&Path>)?),
        vec!["file".to_string(), "sub".to_string()]
    );
    Ok(())
}

#[test]
fn list_dir_lists_root_entries_repeatedly() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    fs::create_dir(dir.path().join("sub"))?;
    let root = NoFollowRoot::new(dir.path())?;

    let expected = vec!["file".to_string(), "sub".to_string()];
    assert_eq!(sorted_names(root.list_dir(None::<&Path>)?), expected);
    assert_eq!(sorted_names(root.list_dir(None::<&Path>)?), expected);
    Ok(())
}

#[test]
fn list_dir_lists_root_entries_concurrently() -> io::Result<()> {
    let dir = tempdir()?;
    let mut expected = Vec::new();
    for i in 0..64 {
        let name = format!("file-{i:02}");
        fs::write(dir.path().join(&name), b"contents")?;
        expected.push(name);
    }
    expected.sort();

    let root = Arc::new(NoFollowRoot::new(dir.path())?);
    let thread_count = 8;
    let iterations = 100;
    let barrier = Arc::new(Barrier::new(thread_count));
    let mut threads = Vec::new();

    for _ in 0..thread_count {
        let root = root.clone();
        let barrier = barrier.clone();
        let expected = expected.clone();
        threads.push(thread::spawn(move || -> io::Result<()> {
            barrier.wait();
            for _ in 0..iterations {
                let names = sorted_names(root.list_dir(None::<&Path>)?);
                if names != expected {
                    return Err(io::Error::other(format!(
                        "unexpected directory listing: {names:?}"
                    )));
                }
            }
            Ok(())
        }));
    }

    for thread in threads {
        thread.join().expect("list_dir thread panicked")?;
    }
    Ok(())
}

#[test]
fn list_dir_lists_child_entries() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir_all(dir.path().join("parent/sub"))?;
    fs::write(dir.path().join("parent/file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert_eq!(
        sorted_names(root.list_dir(Some(Path::new("parent")))?),
        vec!["file".to_string(), "sub".to_string()]
    );
    Ok(())
}

#[test]
fn list_dir_rejects_file_leaf() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.list_dir(Some(Path::new("file"))).is_err());
    Ok(())
}

#[test]
fn list_dir_rejects_leaf_symlink_to_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target"))?;
    fs::write(dir.path().join("target/file"), b"contents")?;
    if !create_dir_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.list_dir(Some(Path::new("link"))).is_err());
    assert_eq!(
        fs::read(dir.path().join("target/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn list_dir_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir_all(dir.path().join("real/child"))?;
    fs::write(dir.path().join("real/child/file"), b"contents")?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.list_dir(Some(Path::new("a/link/child"))).is_err());
    assert_eq!(
        fs::read(dir.path().join("real/child/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn write_file_creates_parents_and_truncates_existing_file() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"old")?;
    let root = NoFollowRoot::new(dir.path())?;

    root.write_file(Path::new("a/b/file"), b"contents", 0o600)?;
    root.write_file(Path::new("file"), b"new", 0o700)?;

    assert_eq!(fs::read(dir.path().join("a/b/file"))?, b"contents".to_vec());
    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    Ok(())
}

#[test]
fn write_file_rejects_existing_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("dir"))?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.write_file(Path::new("dir"), b"contents", 0o600)
            .is_err()
    );
    assert!(
        fs::symlink_metadata(dir.path().join("dir"))?
            .file_type()
            .is_dir()
    );
    Ok(())
}

#[test]
fn write_file_rejects_leaf_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("target"), b"old")?;
    if !create_file_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.write_file(Path::new("link"), b"new", 0o600).is_err());
    assert_eq!(fs::read(dir.path().join("target"))?, b"old".to_vec());
    Ok(())
}

#[test]
fn write_file_rejects_leaf_symlink_to_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target_dir"))?;
    if !create_dir_symlink(Path::new("target_dir"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.write_file(Path::new("link"), b"new", 0o600).is_err());
    assert!(dir.path().join("target_dir").is_dir());
    Ok(())
}

#[test]
fn atomic_replace_file_persists_explicitly() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"old")?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.atomic_replace_file(Path::new("file"), 0o600)?;
    file.write_all(b"new")?;
    file.persist()?;

    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    Ok(())
}

#[test]
fn atomic_replace_file_persist_is_idempotent() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.atomic_replace_file(Path::new("file"), 0o600)?;
    file.write_all(b"contents")?;
    file.persist()?;
    file.persist()?;

    assert_eq!(fs::read(dir.path().join("file"))?, b"contents".to_vec());
    Ok(())
}

#[test]
fn atomic_replace_file_discards_on_drop() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"old")?;
    let root = NoFollowRoot::new(dir.path())?;

    {
        let mut file = root.atomic_replace_file(Path::new("file"), 0o600)?;
        file.write_all(b"new")?;
    }

    assert_eq!(fs::read(dir.path().join("file"))?, b"old".to_vec());
    assert_no_atomic_temp_files(dir.path())?;
    Ok(())
}

#[test]
fn atomic_replace_file_creates_parents() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.atomic_replace_file(Path::new("a/b/file"), 0o600)?;
    file.write_all(b"contents")?;
    file.persist()?;

    assert_eq!(fs::read(dir.path().join("a/b/file"))?, b"contents".to_vec());
    Ok(())
}

#[test]
fn atomic_replace_file_replaces_leaf_symlink_not_target() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("target"), b"old")?;
    if !create_file_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.atomic_replace_file(Path::new("link"), 0o600)?;
    file.write_all(b"new")?;
    file.persist()?;

    assert_eq!(fs::read(dir.path().join("link"))?, b"new".to_vec());
    assert_eq!(fs::read(dir.path().join("target"))?, b"old".to_vec());
    Ok(())
}

#[test]
fn atomic_replace_file_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.atomic_replace_file(Path::new("a/link/file"), 0o600)
            .is_err()
    );
    assert!(!dir.path().join("real/file").exists());
    Ok(())
}

#[test]
fn open_file_reads_regular_file() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    fs::create_dir_all(dir.path().join("a/b"))?;
    fs::write(dir.path().join("a/b/file"), b"nested")?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.open_file(Path::new("file"), OpenFlags::READ, 0)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    assert_eq!(contents, "contents");
    let mut nested_file = root.open_file(Path::new("a/b/file"), OpenFlags::READ, 0)?;
    let mut nested_contents = String::new();
    nested_file.read_to_string(&mut nested_contents)?;
    assert_eq!(nested_contents, "nested");
    Ok(())
}

#[test]
fn open_file_creates_and_truncates_with_flags() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"old contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.open_file(
        Path::new("file"),
        OpenFlags::WRITE | OpenFlags::TRUNCATE,
        0o600,
    )?;
    file.write_all(b"new")?;
    drop(file);

    let mut created = root.open_file(
        Path::new("created"),
        OpenFlags::WRITE | OpenFlags::CREATE_NEW,
        0o600,
    )?;
    created.write_all(b"created")?;
    drop(created);

    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    assert_eq!(fs::read(dir.path().join("created"))?, b"created".to_vec());
    Ok(())
}

#[test]
fn open_file_creates_missing_parent_dirs_with_create_flags() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut file = root.open_file(
        Path::new("a/b/created"),
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
        0o600,
    )?;
    file.write_all(b"created")?;
    drop(file);

    assert_eq!(
        fs::read(dir.path().join("a/b/created"))?,
        b"created".to_vec()
    );
    Ok(())
}

#[test]
fn open_file_create_flag_does_not_imply_write_access() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("existing"), b"old")?;
    let root = NoFollowRoot::new(dir.path())?;

    let mut created = root.open_file(
        Path::new("created"),
        OpenFlags::READ | OpenFlags::CREATE,
        0o600,
    )?;
    assert!(
        created.write_all(b"new").is_err(),
        "READ | CREATE should not grant write access"
    );
    drop(created);

    let mut existing = root.open_file(
        Path::new("existing"),
        OpenFlags::READ | OpenFlags::CREATE,
        0o600,
    )?;
    let mut contents = String::new();
    existing.read_to_string(&mut contents)?;
    assert_eq!(contents, "old");
    assert!(
        existing.write_all(b"new").is_err(),
        "READ | CREATE should not grant write access for existing files"
    );

    assert_eq!(fs::read(dir.path().join("created"))?, b"".to_vec());
    assert_eq!(fs::read(dir.path().join("existing"))?, b"old".to_vec());
    Ok(())
}

#[test]
fn open_flags_x_mode_is_exclusive_write_create() -> io::Result<()> {
    assert_eq!(
        "x".parse::<OpenFlags>()?,
        OpenFlags::WRITE | OpenFlags::CREATE_NEW
    );
    Ok(())
}

#[test]
fn open_file_does_not_create_missing_parent_dirs_without_create_flags() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    let err = root
        .open_file(
            Path::new("a/b/missing"),
            OpenFlags::WRITE | OpenFlags::TRUNCATE,
            0o600,
        )
        .expect_err("open without create flags should not create parent dirs");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(!dir.path().join("a").exists());
    Ok(())
}

#[test]
fn open_file_rejects_leaf_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("target"), b"contents")?;
    if !create_file_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.open_file(Path::new("link"), OpenFlags::READ, 0)
            .is_err()
    );
    Ok(())
}

#[test]
fn open_file_rejects_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("dir"))?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.open_file(Path::new("dir"), OpenFlags::READ, 0)
            .is_err()
    );
    Ok(())
}

#[test]
fn open_file_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::write(dir.path().join("real/file"), b"contents")?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.open_file(Path::new("link/file"), OpenFlags::READ, 0)
            .is_err()
    );
    Ok(())
}

#[test]
fn symlink_metadata_reports_leaf_type() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    fs::create_dir(dir.path().join("dir"))?;
    if !create_file_symlink(Path::new("file"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    let file_metadata = root.symlink_metadata(Some(Path::new("file")))?;
    assert!(file_metadata.is_file());
    assert_eq!(file_metadata.size(), 8);
    let std_file_metadata = LiteMetadata::from(fs::symlink_metadata(dir.path().join("file"))?);
    assert!(std_file_metadata.is_file());
    assert_eq!(std_file_metadata.size(), file_metadata.size());
    let _ = file_metadata.atime();
    let _ = file_metadata.mtime();
    let _ = file_metadata.ctime();
    #[cfg(unix)]
    {
        let std_file_metadata = fs::symlink_metadata(dir.path().join("file"))?;
        assert_eq!(file_metadata.ino(), std_file_metadata.ino());
        assert_eq!(file_metadata.nlink(), std_file_metadata.nlink());
        assert_eq!(file_metadata.uid(), std_file_metadata.uid());
        assert_eq!(file_metadata.gid(), std_file_metadata.gid());
    }
    assert!(root.symlink_metadata(Some(Path::new("dir")))?.is_dir());
    assert!(LiteMetadata::from(fs::symlink_metadata(dir.path().join("dir"))?).is_dir());
    assert!(root.symlink_metadata(Some(Path::new("link")))?.is_symlink());
    assert!(LiteMetadata::from(fs::symlink_metadata(dir.path().join("link"))?).is_symlink());
    assert!(root.symlink_metadata(Some(Path::new("missing"))).is_err());

    let root_metadata = root.symlink_metadata::<&Path>(None)?;
    assert!(root_metadata.is_dir());
    assert_eq!(
        root_metadata.size(),
        LiteMetadata::from(fs::symlink_metadata(dir.path())?).size()
    );
    Ok(())
}

#[test]
fn symlink_metadata_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::write(dir.path().join("real/file"), b"contents")?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.symlink_metadata(Some(Path::new("link/file"))).is_err());
    Ok(())
}

#[test]
fn symlink_metadata_treats_file_parent_as_not_found() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    let err = root
        .symlink_metadata(Some(Path::new("file/child")))
        .expect_err("file parent should not be traversable as a directory");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(
        err.to_string()
            .starts_with("failed to query metadata of symlink `file/child`: "),
        "{err}"
    );
    Ok(())
}

#[test]
fn read_link_reads_leaf_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("target"), b"contents")?;
    if !create_file_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert_eq!(root.read_link(Path::new("link"))?, Path::new("target"));
    Ok(())
}

#[test]
fn read_link_handles_long_target() -> io::Result<()> {
    let dir = tempdir()?;
    let target = PathBuf::from("a".repeat(300));
    if !create_file_symlink(&target, &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert_eq!(root.read_link(Path::new("link"))?, target);
    Ok(())
}

#[test]
fn read_link_rejects_regular_file() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.read_link(Path::new("file")).is_err());
    Ok(())
}

#[test]
fn read_link_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.read_link(Path::new("link/file")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn write_file_uses_requested_mode() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    root.write_file(Path::new("file"), b"contents", 0o700)?;

    assert_eq!(
        fs::metadata(dir.path().join("file"))?.permissions().mode() & 0o777,
        0o700
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn set_permissions_updates_mode() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    root.set_permissions(Path::new("file"), 0o700)?;

    assert_eq!(
        fs::metadata(dir.path().join("file"))?.permissions().mode() & 0o777,
        0o700
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn set_permissions_is_noop_on_windows() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    root.set_permissions(Path::new("file"), 0o700)?;
    assert!(root.set_permissions(Path::new("../escape"), 0o700).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn set_permissions_rejects_leaf_symlink() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir()?;
    fs::write(dir.path().join("target"), b"contents")?;
    fs::set_permissions(dir.path().join("target"), fs::Permissions::from_mode(0o600))?;
    if !create_file_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.set_permissions(Path::new("link"), 0o700).is_err());
    assert_eq!(
        fs::metadata(dir.path().join("target"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn set_permissions_rejects_parent_symlink() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::write(dir.path().join("real/file"), b"contents")?;
    fs::set_permissions(
        dir.path().join("real/file"),
        fs::Permissions::from_mode(0o600),
    )?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.set_permissions(Path::new("link/file"), 0o700).is_err());
    assert_eq!(
        fs::metadata(dir.path().join("real/file"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Ok(())
}

#[test]
fn root_opened_through_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(&dir.path().join("link"))?;

    root.write_file(Path::new("file"), b"contents", 0o600)?;

    assert_eq!(
        fs::read(dir.path().join("real/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn write_symlink_creates_parent_but_does_not_replace_conflicts() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    // Windows symlink creation can require privileges. Treat that as an
    // environment skip for these symlink-specific assertions.
    if !create_file_symlink(Path::new("../target"), &dir.path().join("existing_link"))? {
        return Ok(());
    }
    fs::write(dir.path().join("existing_file"), b"old")?;

    match root.write_symlink(Path::new("a/link"), Path::new("../target")) {
        Ok(()) => {}
        Err(err) => {
            #[cfg(windows)]
            if err.kind() == io::ErrorKind::PermissionDenied || err.raw_os_error() == Some(1314) {
                return Ok(());
            }
            return Err(err);
        }
    }
    assert!(
        root.write_symlink(Path::new("existing_file"), Path::new("target"))
            .is_err()
    );
    assert!(
        root.write_symlink(Path::new("existing_link"), Path::new("target"))
            .is_err()
    );

    assert_eq!(
        fs::read_link(dir.path().join("a/link"))?,
        Path::new("../target")
    );
    assert_eq!(fs::read(dir.path().join("existing_file"))?, b"old".to_vec());
    assert_eq!(
        fs::read_link(dir.path().join("existing_link"))?,
        Path::new("../target")
    );
    Ok(())
}

#[test]
fn write_symlink_to_existing_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target_dir"))?;
    let root = NoFollowRoot::new(dir.path())?;

    match root.write_symlink(Path::new("dir_link"), Path::new("target_dir")) {
        Ok(()) => {}
        Err(err) => {
            #[cfg(windows)]
            if err.kind() == io::ErrorKind::PermissionDenied || err.raw_os_error() == Some(1314) {
                return Ok(());
            }
            return Err(err);
        }
    }

    assert_eq!(
        fs::read_link(dir.path().join("dir_link"))?,
        Path::new("target_dir")
    );
    assert!(dir.path().join("dir_link").is_dir());
    Ok(())
}

#[test]
fn write_symlink_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.write_symlink(Path::new("a/link/new_link"), Path::new("target"))
            .is_err()
    );
    assert!(!dir.path().join("real/new_link").exists());
    Ok(())
}

#[test]
fn write_file_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.write_file(Path::new("link/file"), b"contents", 0o600)
            .is_err()
    );
    assert!(!dir.path().join("real/file").exists());
    Ok(())
}

#[test]
fn write_file_rejects_deep_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/b"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(
        root.write_file(Path::new("a/b/c/d"), b"contents", 0o600)
            .is_err()
    );
    assert!(!dir.path().join("real/c/d").exists());
    Ok(())
}

#[test]
fn remove_file_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::create_dir(dir.path().join("a"))?;
    fs::write(dir.path().join("real/file"), b"contents")?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_file(Path::new("a/link/file")).is_err());
    assert_eq!(
        fs::read(dir.path().join("real/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn remove_dir_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::create_dir(dir.path().join("real/child"))?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_dir(Path::new("a/link/child")).is_err());
    assert!(dir.path().join("real/child").is_dir());
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn openat_no_follow_rejects_symlink_parent_component() -> io::Result<()> {
    use std::ffi::CString;
    use std::fs::File;
    use std::os::fd::AsFd;
    use std::os::fd::FromRawFd;
    use std::os::fd::OwnedFd;

    let dir = tempdir()?;
    fs::create_dir(dir.path().join("a"))?;
    fs::create_dir(dir.path().join("real"))?;
    fs::write(dir.path().join("real/file"), b"contents")?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = File::open(dir.path())?;
    let path = CString::new("a/link/file").unwrap();

    let fd =
        super::unix::openat_no_follow(root.as_fd(), &path, libc::O_RDONLY | libc::O_CLOEXEC, 0);
    if fd >= 0 {
        drop(unsafe { OwnedFd::from_raw_fd(fd) });
        panic!("openat_no_follow unexpectedly traversed a parent symlink");
    }

    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn openat_no_follow_rejects_invalid_components() -> io::Result<()> {
    use std::ffi::CString;
    use std::fs::File;
    use std::os::fd::AsFd;
    use std::os::fd::FromRawFd;
    use std::os::fd::OwnedFd;

    let dir = tempdir()?;
    fs::create_dir(dir.path().join("a"))?;
    let root = File::open(dir.path())?;

    for path in ["", "a//file", "a/../file"] {
        let path = CString::new(path).unwrap();
        let fd =
            super::unix::openat_no_follow(root.as_fd(), &path, libc::O_RDONLY | libc::O_CLOEXEC, 0);
        if fd >= 0 {
            drop(unsafe { OwnedFd::from_raw_fd(fd) });
            panic!("openat_no_follow unexpectedly accepted {:?}", path);
        }
    }

    Ok(())
}

#[test]
fn remove_file_removes_leaf_symlink_not_target() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("target"), b"contents")?;
    if !create_file_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_file(Path::new("link"))?;

    assert!(fs::symlink_metadata(dir.path().join("link")).is_err());
    assert_eq!(fs::read(dir.path().join("target"))?, b"contents".to_vec());
    Ok(())
}

#[test]
fn remove_operations_reject_missing_paths() -> io::Result<()> {
    let dir = tempdir()?;
    let root = NoFollowRoot::new(dir.path())?;

    assert_eq!(
        root.remove_file(Path::new("missing")).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        root.remove_file(Path::new("missing/child"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        root.remove_dir(Path::new("missing")).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        root.remove_dir(Path::new("missing/child"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        root.remove_dir_all(Path::new("missing"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        root.remove_dir_all(Path::new("missing/child"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn remove_file_allows_recreate_while_old_file_is_open_on_windows() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"old")?;
    let mut held = fs::File::open(dir.path().join("file"))?;
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_file(Path::new("file"))?;
    root.write_file(Path::new("file"), b"new", 0o600)?;

    let mut old_contents = String::new();
    held.read_to_string(&mut old_contents)?;
    assert_eq!(old_contents, "old");
    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    drop(held);
    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    Ok(())
}

#[cfg(windows)]
#[test]
fn remove_file_allows_recreate_while_old_file_is_mapped_on_windows() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"old")?;
    let held = fs::File::open(dir.path().join("file"))?;
    let mmap = unsafe { memmap2::Mmap::map(&held)? };
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_file(Path::new("file"))?;
    root.write_file(Path::new("file"), b"new", 0o600)?;

    assert_eq!(mmap.as_ref(), b"old");
    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    drop(mmap);
    drop(held);
    assert_eq!(fs::read(dir.path().join("file"))?, b"new".to_vec());
    Ok(())
}

#[test]
fn remove_file_removes_symlink_to_directory_not_target() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target_dir"))?;
    if !create_dir_symlink(Path::new("target_dir"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_file(Path::new("link"))?;

    assert!(fs::symlink_metadata(dir.path().join("link")).is_err());
    assert!(dir.path().join("target_dir").is_dir());
    Ok(())
}

#[test]
fn remove_dir_removes_empty_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("empty"))?;
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_dir(Path::new("empty"))?;

    assert!(!dir.path().join("empty").exists());
    Ok(())
}

#[test]
fn remove_dir_rejects_non_empty_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("non_empty"))?;
    fs::write(dir.path().join("non_empty/file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_dir(Path::new("non_empty")).is_err());
    assert_eq!(
        fs::read(dir.path().join("non_empty/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn remove_dir_all_removes_tree() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir_all(dir.path().join("tree/a/b"))?;
    fs::write(dir.path().join("tree/file"), b"contents")?;
    fs::write(dir.path().join("tree/a/b/file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_dir_all(Path::new("tree"))?;

    assert!(!dir.path().join("tree").exists());
    Ok(())
}

#[test]
fn remove_dir_all_removes_empty_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("empty"))?;
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_dir_all(Path::new("empty"))?;

    assert!(!dir.path().join("empty").exists());
    Ok(())
}

#[test]
fn remove_dir_all_rejects_file() -> io::Result<()> {
    let dir = tempdir()?;
    fs::write(dir.path().join("file"), b"contents")?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_dir_all(Path::new("file")).is_err());
    assert_eq!(fs::read(dir.path().join("file"))?, b"contents".to_vec());
    Ok(())
}

#[test]
fn remove_dir_all_removes_nested_symlink_not_target() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir_all(dir.path().join("tree/subdir"))?;
    fs::create_dir(dir.path().join("target"))?;
    fs::write(dir.path().join("target/file"), b"contents")?;
    if !create_dir_symlink(Path::new("../target"), &dir.path().join("tree/subdir/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_dir_all(Path::new("tree"))?;

    assert!(!dir.path().join("tree").exists());
    assert_eq!(
        fs::read(dir.path().join("target/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn remove_dir_all_removes_nested_file_symlink_not_target() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("tree"))?;
    fs::write(dir.path().join("target"), b"contents")?;
    if !create_file_symlink(Path::new("../target"), &dir.path().join("tree/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_dir_all(Path::new("tree"))?;

    assert!(!dir.path().join("tree").exists());
    assert_eq!(fs::read(dir.path().join("target"))?, b"contents".to_vec());
    Ok(())
}

#[cfg(windows)]
#[test]
fn remove_dir_all_removes_readonly_file_on_windows() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("tree"))?;
    let file = dir.path().join("tree/file");
    fs::write(&file, b"contents")?;
    let mut permissions = fs::metadata(&file)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions)?;
    let root = NoFollowRoot::new(dir.path())?;

    root.remove_dir_all(Path::new("tree"))?;

    assert!(!dir.path().join("tree").exists());
    Ok(())
}

#[test]
fn remove_dir_all_rejects_leaf_symlink_to_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("target"))?;
    fs::write(dir.path().join("target/file"), b"contents")?;
    if !create_dir_symlink(Path::new("target"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_dir_all(Path::new("link")).is_err());
    assert!(fs::symlink_metadata(dir.path().join("link")).is_ok());
    assert_eq!(
        fs::read(dir.path().join("target/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn remove_dir_all_rejects_parent_symlink() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir_all(dir.path().join("real/tree"))?;
    fs::write(dir.path().join("real/tree/file"), b"contents")?;
    fs::create_dir(dir.path().join("a"))?;
    if !create_dir_symlink(Path::new("../real"), &dir.path().join("a/link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_dir_all(Path::new("a/link/tree")).is_err());
    assert_eq!(
        fs::read(dir.path().join("real/tree/file"))?,
        b"contents".to_vec()
    );
    Ok(())
}

#[test]
fn remove_dir_rejects_symlink_to_directory() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("link"))? {
        return Ok(());
    }
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_dir(Path::new("link")).is_err());
    assert!(dir.path().join("real").is_dir());
    Ok(())
}

#[test]
fn remove_file_does_not_remove_directories() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("dir"))?;
    let root = NoFollowRoot::new(dir.path())?;

    assert!(root.remove_file(Path::new("dir")).is_err());
    assert!(
        fs::symlink_metadata(dir.path().join("dir"))?
            .file_type()
            .is_dir()
    );
    Ok(())
}

#[test]
fn case_insensitive_does_not_bypass_symlink_rejection() -> io::Result<()> {
    let dir = tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    if !create_dir_symlink(Path::new("real"), &dir.path().join("Link"))? {
        return Ok(());
    }

    // Skip on case-sensitive filesystems where "LINK" != "Link".
    if fs::symlink_metadata(dir.path().join("LINK")).is_err() {
        return Ok(());
    }

    let root = NoFollowRoot::new(dir.path())?;
    assert!(
        root.write_file(Path::new("LINK/file"), b"x", 0o600)
            .is_err()
    );
    assert!(!dir.path().join("real/file").exists());
    Ok(())
}

fn create_file_symlink(target: &Path, link: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }
    #[cfg(windows)]
    {
        match std::os::windows::fs::symlink_file(target, link) {
            Ok(()) => Ok(true),
            // Count permission-limited Windows environments as skipped by the
            // caller instead of failing unrelated no-follow tests.
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => Ok(false),
            Err(err) => Err(err),
        }
    }
}

fn create_dir_symlink(target: &Path, link: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }
    #[cfg(windows)]
    {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(true),
            // Count permission-limited Windows environments as skipped by the
            // caller instead of failing unrelated no-follow tests.
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => Ok(false),
            Err(err) => Err(err),
        }
    }
}

fn assert_no_atomic_temp_files(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        assert!(
            !name.to_string_lossy().starts_with(".no-follow-atomic."),
            "temporary file should be cleaned up: {:?}",
            name
        );
    }
    Ok(())
}

fn sorted_names(names: Vec<std::ffi::OsString>) -> Vec<String> {
    let mut names: Vec<_> = names
        .into_iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
