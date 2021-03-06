/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    fs::{create_dir_all, remove_dir, remove_dir_all, symlink_metadata, File},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use std::fs::{Metadata, OpenOptions};

#[cfg(not(windows))]
use std::{
    fs::{set_permissions, Permissions},
    os::unix::fs::PermissionsExt,
};

use anyhow::{bail, ensure, Context, Result};

use fsinfo::{fstype, FsType};
use types::RepoPath;
use util::path::remove_file;

use crate::pathauditor::PathAuditor;

use once_cell::sync::Lazy;

#[derive(Clone)]
pub struct VFS {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    auditor: PathAuditor,
    supports_symlinks: bool,
    supports_executables: bool,
}

#[derive(Clone, Copy)]
pub enum UpdateFlag {
    Symlink,
    Executable,
}

#[cfg(unix)]
static UMASK: Lazy<u32> = Lazy::new(|| unsafe {
    let umask = libc::umask(0);
    libc::umask(umask);
    #[allow(clippy::useless_conversion)] // mode_t is u16 on mac and u32 on linux
    umask.into()
});

impl VFS {
    pub fn new(root: PathBuf) -> Result<Self> {
        let auditor = PathAuditor::new(&root);
        let fs_type =
            fstype(&root).with_context(|| format!("Can't construct a VFS for {:?}", root))?;
        let supports_symlinks = supports_symlinks(&fs_type);
        let supports_executables = supports_executables(&fs_type);

        Ok(Self {
            inner: Arc::new(Inner {
                root,
                auditor,
                supports_symlinks,
                supports_executables,
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn join(&self, path: &RepoPath) -> PathBuf {
        self.inner.root.join(path.as_str())
    }

    pub fn metadata(&self, path: &RepoPath) -> Result<Metadata> {
        self.join(path).symlink_metadata().map_err(|e| e.into())
    }

    /// The file `path` can't be written to, attempt to fixup the directories and files so the file can
    /// be created.
    ///
    /// This is a slow operation, and should not be called before attempting to create `path`.
    fn clear_conflicts(&self, path: &RepoPath) -> Result<()> {
        let filepath = self.inner.auditor.audit(path)?;
        let mut path = filepath.as_path();
        if let Ok(metadata) = symlink_metadata(path) {
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                remove_dir_all(path)
                    .with_context(|| format!("Can't remove directory {:?}", path))?;
            }
        }

        loop {
            if path == self.inner.root {
                break;
            }

            if let Ok(metadata) = symlink_metadata(path) {
                let file_type = metadata.file_type();
                if file_type.is_file() || file_type.is_symlink() {
                    remove_file(path).with_context(|| format!("Can't remove file {:?}", path))?;
                }
            }

            // By virtue of the fact that we haven't reached the root, we are guaranteed to
            // have a parent directory.
            path = path.parent().unwrap();
        }

        let dir = filepath.parent().unwrap();
        create_dir_all(dir).with_context(|| format!("Can't create directory {:?}", dir))?;

        Ok(())
    }

    fn write_mode(&self, filepath: &Path, content: &[u8], exec: bool) -> Result<usize> {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }

        let mut f = options.open(filepath)?;

        #[cfg(unix)]
        {
            let metadata = f.metadata()?;
            let mut permissions = metadata.permissions();
            let mode = Self::update_mode(permissions.mode(), exec);
            permissions.set_mode(mode);
            f.set_permissions(permissions)
                .with_context(|| format!("Failed to set permissions on {:?}", filepath))?;
        }

        f.write_all(content)
            .with_context(|| format!("Can't write to {:?}", filepath))?;
        Ok(content.len())
    }

    #[cfg(unix)]
    fn update_mode(mode: u32, exec: bool) -> u32 {
        if exec {
            mode | (mode & 0o444) >> 2 & !*UMASK
        } else {
            mode & 0o666
        }
    }

    fn set_exec(&self, filepath: &Path, flag: bool) -> Result<()> {
        #[cfg(windows)]
        return Ok(());

        #[cfg(not(windows))]
        {
            let mode = if flag { 0o755 } else { 0o644 };
            let perms = Permissions::from_mode(mode);
            set_permissions(filepath, perms)
                .with_context(|| format!("Can't update exec flag({}) on {:?}", flag, filepath))?;
            Ok(())
        }
    }

    /// On some OS/filesystems, symlinks aren't supported, we simply create a file where it's content
    /// is the symlink destination for these.
    fn plain_symlink_file(link_name: &Path, link_dest: &Path) -> Result<()> {
        let link_dest = match link_dest.to_str() {
            None => bail!("Not a valid UTF-8 path: {:?}", link_dest),
            Some(s) => s,
        };

        Ok(File::create(link_name)?.write_all(link_dest.as_bytes())?)
    }

    /// Add a symlink `link_name` pointing to `link_dest`. On platforms that do not support symlinks,
    /// `link_name` will be a file containing the path to `link_dest`.
    fn symlink(&self, link_name: &Path, link_dest: &Path) -> Result<()> {
        #[cfg(windows)]
        let result = Self::plain_symlink_file(link_name, link_dest);

        #[cfg(not(windows))]
        let result = if self.inner.supports_symlinks {
            std::os::unix::fs::symlink(link_dest, link_name).map_err(Into::into)
        } else {
            Self::plain_symlink_file(link_name, link_dest)
        };

        result.with_context(|| format!("Can't create symlink '{:?} -> {:?}'", link_name, link_dest))
    }

    /// Write a symlink file at `filepath`. The destination is represented by `content`.
    fn write_symlink(&self, filepath: &Path, content: &[u8]) -> Result<usize> {
        let link_dest = Path::new(std::str::from_utf8(content)?);

        self.symlink(filepath, link_dest)?;
        Ok(filepath.as_os_str().len())
    }

    /// Overwrite the content of the file at `path` with `data`. The number of bytes written on
    /// disk will be returned.
    fn write_inner(
        &self,
        path: &RepoPath,
        data: &[u8],
        flags: Option<UpdateFlag>,
    ) -> Result<usize> {
        let filepath = self
            .inner
            .auditor
            .audit(path)
            .with_context(|| format!("Can't write into {}", path))?;

        match flags {
            None => self.write_mode(&filepath, data, false),
            Some(UpdateFlag::Executable) => self.write_mode(&filepath, data, true),
            Some(UpdateFlag::Symlink) => self.write_symlink(&filepath, data),
        }
    }

    /// Overwrite content of the file, try to clear conflicts if attempt fails
    ///
    /// Return an error if fails to overwrite after clearing conflicts, or if clear conflicts fail
    pub fn write(&self, path: &RepoPath, data: &[u8], flag: Option<UpdateFlag>) -> Result<usize> {
        // Fast path: let's try to open the file directly, we'll handle the failure only if this fails.
        match self.write_inner(path, data, flag) {
            Ok(size) => Ok(size),
            Err(e) => {
                // Ideally, we shouldn't need to retry for some failures, but this is the slow path, any
                // failures not due to a conflicting file would show up here again, so let's not worry
                // about it.
                self.clear_conflicts(path).with_context(|| {
                    format!("Can't clear conflicts after handling error \"{:?}\"", e)
                })?;
                self.write_inner(path, data, flag)
                    .with_context(|| format!("Can't write after handling error \"{:?}\"", e))
            }
        }
    }

    pub fn set_executable(&self, path: &RepoPath, flag: bool) -> Result<()> {
        let filepath = self
            .inner
            .auditor
            .audit(path)
            .with_context(|| format!("Can't write into {}", path))?;

        self.set_exec(&filepath, flag)
    }

    /// Remove the file at `path`.
    ///
    /// If file does not exist, returns without an error
    ///
    /// The parent directories of this file will be removed recursively if they are empty.
    pub fn remove(&self, path: &RepoPath) -> Result<()> {
        let mut filepath = self.inner.auditor.audit(path)?;
        self.remove_keep_path(&filepath)?;

        // Mercurial doesn't track empty directories, remove them
        // recursively.
        loop {
            if !filepath.pop() || filepath == self.inner.root {
                break;
            }

            if remove_dir(&filepath).is_err() {
                break;
            }
        }
        Ok(())
    }

    /// Removes file, but inlike Self::remove, does not delete empty directories.
    fn remove_keep_path(&self, filepath: &PathBuf) -> Result<()> {
        if let Ok(metadata) = symlink_metadata(&filepath) {
            let file_type = metadata.file_type();
            if file_type.is_file() || file_type.is_symlink() {
                let result = remove_file(&filepath)
                    .with_context(|| format!("Can't remove file {:?}", filepath));
                if let Err(e) = result {
                    if let Some(io_error) = e.downcast_ref::<io::Error>() {
                        ensure!(io_error.kind() == ErrorKind::NotFound, e);
                    } else {
                        return Err(e);
                    };
                }
            }
        }

        Ok(())
    }

    pub fn supports_symlinks(&self) -> bool {
        self.inner.supports_symlinks
    }

    pub fn supports_executables(&self) -> bool {
        self.inner.supports_executables
    }
}

#[cfg(unix)]
#[cfg(test)]
mod unix_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_symlink_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a").unwrap();
        vfs.write(path, b"abc", Some(UpdateFlag::Symlink)).unwrap();
        vfs.write(path, &[1, 2, 3], None).unwrap();
        let mut buf = tmp.path().to_path_buf();
        buf.push("a");
        let metadata = fs::symlink_metadata(buf).unwrap();
        assert!(metadata.file_type().is_file())
    }

    #[test]
    fn test_exec_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a").unwrap();
        vfs.write(path, "abc".as_bytes(), Some(UpdateFlag::Executable))
            .unwrap();
        vfs.write(path, &[1, 2, 3], None).unwrap();
        let mut buf = tmp.path().to_path_buf();
        buf.push("a");
        let metadata = fs::symlink_metadata(buf).unwrap();
        assert_eq!(0, metadata.permissions().mode() & 0o111)
    }

    #[test]
    fn test_update_mode() {
        assert_eq!(0o644, VFS::update_mode(0o644, false));
        assert_eq!(0o755, VFS::update_mode(0o755, true));

        assert_eq!(0o755, VFS::update_mode(0o644, true));
        assert_eq!(0o644, VFS::update_mode(0o755, false));
    }
}

/// Since Windows doesn't support symlinks (without Windows' Developer Mode), and NTFS on unices is
/// only used for repos that are intended to be used on Windows, pretend that NTFS doesn't support
/// symlinks. This is of course a lie since unices have no issues supporting symlinks on NTFS.
///
/// Once the need to use NTFS on unices is gone (because this module solves the slowness), this
/// hack will be removed.
fn supports_symlinks(fs_type: &FsType) -> bool {
    match *fs_type {
        FsType::NTFS => false,
        // TODO(T66590035): Once EdenFS on Windows support symlink, remove this
        FsType::EDENFS => !cfg!(windows),
        _ => true,
    }
}

/// Since Windows determines if a file is executable based on its extension, it doesn't support
/// marking files as executable.
fn supports_executables(fs_type: &FsType) -> bool {
    match *fs_type {
        FsType::NTFS => false,
        FsType::EDENFS => !cfg!(windows),
        _ => true,
    }
}

pub fn is_executable(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    return metadata.permissions().mode() & 0o111 != 0;

    #[cfg(target_os = "windows")]
    {
        let _ = metadata;
        panic!("is_executable is not supported on Windows");
    }
}

pub fn is_symlink(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    return metadata.file_type().is_symlink();

    #[cfg(target_os = "windows")]
    {
        let _ = metadata;
        panic!("is_symlink is not supported on Windows");
    }
}
