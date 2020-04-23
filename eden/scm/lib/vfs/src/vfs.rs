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
};

#[cfg(not(windows))]
use std::{
    fs::{set_permissions, Permissions},
    os::unix::fs::PermissionsExt,
};

use anyhow::{bail, ensure, Context, Result};
use bytes::Bytes;

use fsinfo::{fstype, FsType};
use types::RepoPath;
use util::path::remove_file;

use crate::pathauditor::PathAuditor;

#[derive(Clone)]
pub struct VFS {
    root: PathBuf,
    auditor: PathAuditor,
    can_symlink: bool,
}

#[derive(Clone, Copy)]
pub enum UpdateFlag {
    Symlink,
    Executable,
}

impl VFS {
    pub fn new(root: PathBuf) -> Result<Self> {
        let auditor = PathAuditor::new(&root);
        let can_symlink = supports_symlinks(&root)
            .with_context(|| format!("Can't construct a VFS for {:?}", root))?;

        Ok(Self {
            root,
            auditor,
            can_symlink,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file `path` can't be written to, attempt to fixup the directories and files so the file can
    /// be created.
    ///
    /// This is a slow operation, and should not be called before attempting to create `path`.
    pub fn clear_conflicts(&self, path: &RepoPath) -> Result<()> {
        let filepath = self.auditor.audit(path)?;
        let mut path = filepath.as_path();
        if let Ok(metadata) = symlink_metadata(path) {
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                remove_dir_all(path)
                    .with_context(|| format!("Can't remove directory {:?}", path))?;
            }
        }

        loop {
            if path == self.root {
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

    /// Write a plain file with `content` at `filepath`.
    fn write_regular(&self, filepath: &Path, content: &Bytes) -> Result<usize> {
        let mut f = File::create(&filepath)?;
        f.write_all(&content)
            .with_context(|| format!("Can't write to {:?}", filepath))?;
        Ok(content.len())
    }

    /// Write an executable file with `content` as `filepath`.
    fn write_executable(&self, filepath: &Path, content: &Bytes) -> Result<usize> {
        let size = self.write_regular(filepath, content)?;

        #[cfg(windows)]
        return Ok(size);

        #[cfg(not(windows))]
        {
            let perms = Permissions::from_mode(0o755);
            set_permissions(filepath, perms)
                .with_context(|| format!("Can't set {:?} as executable", filepath))?;
            Ok(size)
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
        let result = if self.can_symlink {
            std::os::unix::fs::symlink(link_dest, link_name).map_err(Into::into)
        } else {
            Self::plain_symlink_file(link_name, link_dest)
        };

        result.with_context(|| format!("Can't create symlink '{:?} -> {:?}'", link_name, link_dest))
    }

    /// Write a symlink file at `filepath`. The destination is represented by `content`.
    fn write_symlink(&self, filepath: &Path, content: &Bytes) -> Result<usize> {
        let link_dest = Path::new(std::str::from_utf8(content.as_ref())?);

        self.symlink(filepath, link_dest)?;
        Ok(filepath.as_os_str().len())
    }

    /// Overwrite the content of the file at `path` with `data`. The number of bytes written on
    /// disk will be returned.
    pub fn write(&self, path: &RepoPath, data: &Bytes, flags: Option<UpdateFlag>) -> Result<usize> {
        let filepath = self
            .auditor
            .audit(path)
            .with_context(|| format!("Can't write into {}", path))?;

        match flags {
            None => self.write_regular(&filepath, data),
            Some(UpdateFlag::Executable) => self.write_executable(&filepath, data),
            Some(UpdateFlag::Symlink) => self.write_symlink(&filepath, data),
        }
    }

    /// Remove the file at `path`.
    ///
    /// The parent directories of this file will be removed recursively if they are empty.
    pub fn remove(&self, path: &RepoPath) -> Result<()> {
        let mut filepath = self.auditor.audit(path)?;

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

        // Mercurial doesn't track empty directories, remove them
        // recursively.
        loop {
            if !filepath.pop() || filepath == self.root {
                break;
            }

            if remove_dir(&filepath).is_err() {
                break;
            }
        }
        Ok(())
    }
}

/// Since Windows doesn't support symlinks (without Windows' Developer Mode), and NTFS on unices is
/// only used for repos that are intended to be used on Windows, pretend that NTFS doesn't support
/// symlinks. This is of course a lie since unices have no issues supporting symlinks on NTFS.
///
/// Once the need to use NTFS on unices is gone (because this module solves the slowness), this
/// hack will be removed.
fn supports_symlinks(path: &Path) -> Result<bool> {
    Ok(fstype(path)? != FsType::NTFS)
}
