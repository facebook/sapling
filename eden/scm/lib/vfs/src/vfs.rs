/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::Metadata;
use std::io;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use bitflags::bitflags;
use fsinfo::FsType;
use fsinfo::fstype;
use minibytes::Bytes;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;
use util::no_follow::AtomicReplaceFile;
use util::no_follow::LiteMetadata;
use util::no_follow::NoFollowRoot;
use util::no_follow::OpenFlags;
use util::no_follow::normalize_not_directory;
use util::path::symlink_file;

use crate::pathauditor::PathAuditor;

/// The type of conflict encountered when `clear_conflicts` is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictType {
    File,
    Symlink,
    Directory,
}

impl std::fmt::Display for ConflictType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConflictType::File => write!(f, "file"),
            ConflictType::Symlink => write!(f, "symlink"),
            ConflictType::Directory => write!(f, "directory"),
        }
    }
}

/// Error returned when `clear_conflicts` is disabled and a conflict is detected.
#[derive(Debug)]
pub struct ClearConflictError {
    /// The path that we were trying to write to.
    pub target_path: PathBuf,
    /// The conflicting path that is blocking the write.
    pub conflict_path: PathBuf,
    /// The type of conflict.
    pub conflict_type: ConflictType,
}

impl std::fmt::Display for ClearConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot write to {:?}: conflicting {} exists at {:?}",
            self.target_path, self.conflict_type, self.conflict_path
        )
    }
}

impl std::error::Error for ClearConflictError {}

fn normalize_not_directory_anyhow(err: anyhow::Error) -> anyhow::Error {
    match err.downcast::<io::Error>() {
        Ok(err) => normalize_not_directory(err).into(),
        Err(err) => err,
    }
}

fn path_component_from_dir_entry(name: std::ffi::OsString) -> Result<PathComponentBuf> {
    let name = name
        .into_string()
        .map_err(|name| anyhow::format_err!("directory entry is not UTF-8: {name:?}"))?;
    Ok(PathComponentBuf::from_string(name)?)
}

#[derive(Clone)]
pub struct VFS {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    // Lazily initialized to better support use-cases that vfs root isn't present during
    // initialization.
    no_follow: OnceLock<NoFollowRoot>,
    auditor: PathAuditor,
    // For unknown (or undecided, e.g. fuse) fstype, lazily initialized to delay
    // or avoid real detection temp file side effect.
    supports_symlinks: OnceLock<bool>,
    supports_executables: bool,
    case_sensitive: bool,
    /// Whether to automatically blow away conflicting paths/directories in order to successfully
    /// write a file.
    overwrite_path_conflicts: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum UpdateFlag {
    Regular,
    Symlink,
    Executable,
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct RemoveOptions: u8 {
        /// Return success if `path` does not exist.
        const IGNORE_MISSING_PATH = 1 << 0;
        /// Return success if the existing leaf is not a regular file or symlink.
        ///
        /// Symlinks are classified without following them. This requires a metadata
        /// check before removal; that check is not atomic with the later removal, so
        /// a concurrent change after classification can still make removal fail.
        const IGNORE_NON_FILE_OR_SYMLINK = 1 << 1;
        /// Remove empty parent directories after handling the leaf.
        const PRUNE_EMPTY_PARENTS = 1 << 2;
    }
}

impl VFS {
    /// Create a new VFS instance with non-destructive conflict handling.
    ///
    /// When conflicts are encountered (e.g., a file exists where a directory is needed),
    /// operations will return an error instead of removing the conflicting files.
    pub fn new(root: PathBuf) -> Result<Self> {
        Self::new_inner(root, false)
    }

    /// Create a new VFS instance with destructive conflict handling.
    ///
    /// When conflicts are encountered (e.g., a file exists where a directory is needed),
    /// conflicting files/symlinks/directories will be automatically removed.
    pub fn new_destructive(root: PathBuf) -> Result<Self> {
        Self::new_inner(root, true)
    }

    fn new_inner(root: PathBuf, overwrite_path_conflicts: bool) -> Result<Self> {
        let fs_type = fstype(&root)
            .map_err(normalize_not_directory_anyhow)
            .with_context(|| format!("can't construct a VFS for {root:?}"))?;
        let supports_executables = supports_executables(&fs_type);
        let known_symlink_support = if std::env::var_os("SL_DEBUG_DISABLE_SYMLINKS").is_some() {
            Some(false)
        } else {
            known_symlink_support(&fs_type)
        };
        let case_sensitive =
            case_sensitive(&root, &fs_type).map_err(normalize_not_directory_anyhow)?;
        let no_follow = OnceLock::new();
        let auditor = PathAuditor::new(&root, case_sensitive);

        Ok(Self {
            inner: Arc::new(Inner {
                root,
                no_follow,
                auditor,
                supports_symlinks: known_symlink_support
                    .map(OnceLock::from)
                    .unwrap_or_default(),
                supports_executables,
                case_sensitive,
                overwrite_path_conflicts,
            }),
        })
    }

    fn no_follow(&self) -> Result<&NoFollowRoot> {
        if let Some(no_follow) = self.inner.no_follow.get() {
            Ok(no_follow)
        } else {
            let root = NoFollowRoot::new(self.root())?;
            Ok(self.inner.no_follow.get_or_init(|| root))
        }
    }

    /// Return the raw no-follow root backing this VFS.
    ///
    /// This is a low-level escape hatch for callers that intentionally need to
    /// bypass [`PathAuditor`] while preserving no-follow path traversal. Normal
    /// working-copy file operations should use the VFS methods instead. This
    /// is intended for narrow sentinel/probing cases, such as checking for a
    /// nested repo marker below a directory.
    pub fn raw_no_follow_root(&self) -> Result<&NoFollowRoot> {
        self.no_follow()
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn case_sensitive(&self) -> bool {
        self.inner.case_sensitive
    }

    pub fn join(&self, path: &RepoPath) -> PathBuf {
        self.inner.root.join(path.as_str())
    }

    /// Open an existing directory below this root as a new VFS root.
    pub fn open_vfs(&self, path: &RepoPath) -> Result<Self> {
        if path.is_empty() {
            return Ok(self.clone());
        }

        self.inner.auditor.audit_components(path)?;

        let root = self.join(path);
        let no_follow = self.no_follow()?.open_root(path)?;
        Ok(Self {
            inner: Arc::new(Inner {
                root: root.clone(),
                no_follow: OnceLock::from(no_follow),
                auditor: PathAuditor::new(&root, self.inner.case_sensitive),
                supports_symlinks: self
                    .inner
                    .supports_symlinks
                    .get()
                    .copied()
                    .map(OnceLock::from)
                    .unwrap_or_default(),
                supports_executables: self.inner.supports_executables,
                case_sensitive: self.inner.case_sensitive,
                overwrite_path_conflicts: self.inner.overwrite_path_conflicts,
            }),
        })
    }

    /// Create a directory at `path` without following symlinks.
    pub fn create_dir(&self, path: &RepoPath, mode: Option<u32>) -> Result<()> {
        self.inner.auditor.audit_components(path)?;
        Ok(self.no_follow()?.create_dir(path, mode)?)
    }

    /// Create a directory and all missing parent directories at `path` without
    /// following symlinks.
    pub fn create_dir_all(&self, path: &RepoPath, mode: Option<u32>) -> Result<()> {
        if path.is_empty() {
            return Ok(());
        }

        self.inner.auditor.audit_components(path)?;
        Ok(self.no_follow()?.create_dir_all(path, mode)?)
    }

    /// List names in `path` without following symlinks.
    pub fn list_dir(&self, path: &RepoPath) -> Result<Vec<Result<PathComponentBuf>>> {
        if !path.is_empty() {
            self.inner.auditor.audit_components(path)?;
        }

        Ok(self
            .no_follow()?
            .list_dir((!path.is_empty()).then_some(path))?
            .into_iter()
            .map(path_component_from_dir_entry)
            .collect())
    }

    pub fn metadata(&self, path: &RepoPath) -> Result<LiteMetadata> {
        tracing::trace!(?path, "fetching metadata");

        if !path.is_empty() {
            self.inner.auditor.audit_components(path)?;
        }
        self.no_follow()?
            .symlink_metadata((!path.is_empty()).then_some(path))
            .map_err(Into::into)
    }

    pub fn exists(&self, path: &RepoPath) -> Result<bool> {
        match self.metadata(path) {
            Ok(_) => Ok(true),
            Err(err)
                if err
                    .downcast_ref::<io::Error>()
                    .is_some_and(|err| err.kind() == ErrorKind::NotFound) =>
            {
                Ok(false)
            }
            Err(err) => Err(err),
        }
    }

    pub fn is_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(self.metadata(path)?.is_file())
    }

    /// Open the regular file at `path` without following symlinks.
    /// `mode` is only used for newly created files.
    pub fn open(&self, path: &RepoPath, flags: OpenFlags, mode: u32) -> Result<File> {
        self.inner.auditor.audit_components(path)?;
        let mode = util::file::apply_umask(mode);
        let no_follow = self.no_follow()?;

        if flags.contains(OpenFlags::CREATE | OpenFlags::TRUNCATE) {
            if let Ok(metadata) = no_follow.symlink_metadata(Some(path)) {
                let should_remove = (metadata.is_file()
                    && (metadata.mode() & 0o777) != (mode & 0o777))
                    || metadata.is_symlink();
                if should_remove {
                    match no_follow.remove_file(path) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::NotFound => {}
                        Err(err) => return Err(err.into()),
                    }
                }
            }
        }

        match no_follow.open_file(path, flags, mode) {
            Ok(file) => Ok(file),
            Err(err) if flags.creates_file() => {
                let err = anyhow::Error::from(err);
                self.clear_conflicts(path).with_context(|| {
                    format!("can't clear conflicts after handling error \"{err:#}\"")
                })?;
                Ok(no_follow.open_file(path, flags, mode)?)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Open a temporary file that atomically replaces `path` when persisted.
    pub fn open_with_atomic_replace(
        &self,
        path: &RepoPath,
        mode: u32,
    ) -> Result<AtomicReplaceFile> {
        self.inner.auditor.audit_components(path)?;
        Ok(self.no_follow()?.atomic_replace_file(path, mode)?)
    }

    /// The file `path` can't be written to, attempt to fixup the directories and files so the file can
    /// be created.
    ///
    /// This is a slow operation, and should not be called before attempting to create `path`.
    ///
    /// If `clear_conflicts` is disabled via `overwrite_path_conflicts=false`, this will return an error
    /// with information about the conflict instead of removing the conflicting files.
    fn clear_conflicts(&self, repo_path: &RepoPath) -> Result<()> {
        let full_path = self.join(repo_path);
        let clear_conflicts_enabled = self.inner.overwrite_path_conflicts;

        // Walk down our ancestors, removing the first regular file or symlink
        // we find. This is currently best-effort and has an inherent
        // stat/remove TOCTOU window: another process can replace the path after
        // `symlink_metadata` returns. Parent traversal and removals go through
        // NoFollowRoot, and callers retry the actual write/open through
        // NoFollowRoot too, so a racy replacement symlink is rejected or removed
        // as a leaf instead of being followed.
        let mut prefix = RepoPathBuf::new();
        for part in repo_path.components() {
            prefix.push(part);
            let conflict_path = self.join(&prefix);

            let metadata = match self.no_follow()?.symlink_metadata(Some(&prefix)) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == ErrorKind::NotFound => break,
                Err(err) => return Err(err.into()),
            };

            if metadata.is_file() || metadata.is_symlink() {
                if !clear_conflicts_enabled {
                    let conflict_type = if metadata.is_symlink() {
                        ConflictType::Symlink
                    } else {
                        ConflictType::File
                    };
                    return Err(ClearConflictError {
                        target_path: full_path,
                        conflict_path,
                        conflict_type,
                    }
                    .into());
                }
                self.no_follow()?.remove_file(&prefix)?;
                break;
            }

            // If the full destination is a directory, clear it out.
            if metadata.is_dir() && prefix.as_repo_path() == repo_path {
                if !clear_conflicts_enabled {
                    return Err(ClearConflictError {
                        target_path: full_path,
                        conflict_path,
                        conflict_type: ConflictType::Directory,
                    }
                    .into());
                }
                self.no_follow()?.remove_dir_all(&prefix).with_context(|| {
                    format!("can't remove conflicting directory {conflict_path:?}")
                })?;
                break;
            }
        }

        Ok(())
    }

    fn write_mode(&self, path: &RepoPath, content: blob::Blob, exec: bool) -> Result<usize> {
        let bytes = content.into_bytes();

        #[cfg(windows)]
        let _ = exec;

        #[cfg(unix)]
        let existing_mode = self
            .no_follow()?
            .symlink_metadata(Some(path))
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.mode() & 0o7777);

        #[cfg(unix)]
        let create_mode = Self::update_mode(util::file::apply_umask(0o666), exec);
        #[cfg(windows)]
        let create_mode = 0o666;

        self.no_follow()?.write_file(path, &bytes, create_mode)?;

        #[cfg(unix)]
        if let Some(existing_mode) = existing_mode {
            let mode = Self::update_mode(existing_mode, exec);
            if mode != existing_mode {
                self.no_follow()?.set_permissions(path, mode)?;
            }
        }

        Ok(bytes.len())
    }

    #[cfg(unix)]
    fn update_mode(mode: u32, exec: bool) -> u32 {
        if exec {
            mode | util::file::apply_umask((mode & 0o444) >> 2)
        } else {
            mode & 0o666
        }
    }

    #[cfg(windows)]
    fn set_exec(&self, path: &RepoPath, _: bool) -> Result<()> {
        self.no_follow()?.set_permissions(path, 0o666)?;
        return Ok(());
    }

    #[cfg(unix)]
    fn set_exec(&self, path: &RepoPath, flag: bool) -> Result<()> {
        let mode = self.no_follow()?.symlink_metadata(Some(path))?.mode() & 0o7777;
        let mode = Self::update_mode(mode, flag);
        self.no_follow()?.set_permissions(path, mode)?;
        Ok(())
    }

    /// On some OS/filesystems, symlinks aren't supported, we simply create a file where it's content
    /// is the symlink destination for these.
    fn plain_symlink_file(&self, link_name: &RepoPath, link_dest: &Path) -> Result<()> {
        let link_dest = match link_dest.to_str() {
            None => bail!("not a valid UTF-8 path: {link_dest:?}"),
            Some(s) => s,
        };

        Ok(self
            .no_follow()?
            .write_file(link_name, link_dest.as_bytes(), 0o666)?)
    }

    /// Add a symlink `link_name` pointing to `link_dest`. On platforms that do not support symlinks,
    /// `link_name` will be a file containing the path to `link_dest`.
    fn symlink(&self, link_name: &RepoPath, link_dest: &Path) -> Result<()> {
        if self.supports_symlinks() && (cfg!(unix) || cfg!(windows)) {
            #[cfg(windows)]
            {
                self.no_follow()?.write_symlink(
                    link_name,
                    util::path::replace_slash_with_backslash(link_dest).as_path(),
                )?;
                Ok(())
            }
            #[cfg(unix)]
            {
                self.no_follow()?.write_symlink(link_name, link_dest)?;
                Ok(())
            }
        } else {
            self.plain_symlink_file(link_name, link_dest)
        }
    }

    /// Write a symlink file at `filepath`. The destination is represented by `content`.
    fn write_symlink(&self, path: &RepoPath, content: blob::Blob) -> Result<usize> {
        // This is zero-copy assuming blob contains a Bytes.
        let content = content.to_bytes();
        let link_dest = Path::new(std::str::from_utf8(content.as_ref())?);

        self.symlink(path, link_dest)?;
        Ok(self.join(path).as_os_str().len())
    }

    /// Overwrite the content of the file at `path` with `data`. The number of bytes written on
    /// disk will be returned.
    fn write_inner(&self, path: &RepoPath, data: blob::Blob, flags: UpdateFlag) -> Result<usize> {
        self.inner.auditor.audit_components(path)?;

        match flags {
            UpdateFlag::Regular => self.write_mode(path, data, false),
            UpdateFlag::Executable => self.write_mode(path, data, true),
            UpdateFlag::Symlink => self.write_symlink(path, data),
        }
    }

    /// Overwrite content of the file, try to clear conflicts if attempt fails
    ///
    /// Return an error if fails to overwrite after clearing conflicts, or if clear conflicts fail
    pub fn write(&self, path: &RepoPath, data: blob::Blob, flag: UpdateFlag) -> Result<usize> {
        // Fast path: let's try to open the file directly, we'll handle the failure only if this fails.
        match self.write_inner(path, data.clone(), flag) {
            Ok(size) => Ok(size),
            Err(e) => {
                // Ideally, we shouldn't need to retry for some failures, but this is the slow path, any
                // failures not due to a conflicting file would show up here again, so let's not worry
                // about it.
                self.clear_conflicts(path).with_context(|| {
                    format!("can't clear conflicts after handling error \"{e:#}\"")
                })?;

                self.write_inner(path, data, flag)
            }
        }
    }

    pub fn set_executable(&self, path: &RepoPath, flag: bool) -> Result<()> {
        self.inner.auditor.audit_components(path)?;

        self.set_exec(path, flag)
    }

    pub fn set_permissions(&self, path: &RepoPath, mode: u32) -> Result<()> {
        self.inner.auditor.audit_components(path)?;
        self.no_follow()?.set_permissions(path, mode)?;
        Ok(())
    }

    /// Remove a file or symlink at `path`.
    pub fn remove(&self, path: &RepoPath, options: RemoveOptions) -> Result<()> {
        self.inner.auditor.audit_components(path)?;
        self.remove_leaf(path, options)?;

        if options.contains(RemoveOptions::PRUNE_EMPTY_PARENTS) {
            // Mercurial doesn't track empty directories, remove them
            // recursively.
            let mut parent = path.to_owned();
            loop {
                if !parent.pop() || parent.is_empty() {
                    break;
                }

                if self.no_follow()?.remove_dir(&parent).is_err() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn remove_leaf(&self, path: &RepoPath, options: RemoveOptions) -> Result<()> {
        let no_follow = self.no_follow()?;

        if options.contains(RemoveOptions::IGNORE_NON_FILE_OR_SYMLINK) {
            // The metadata check is only used to classify the current leaf for
            // ignore options. The leaf may race and change before removal;
            // NoFollowRoot::remove_file still removes the final component as a
            // leaf and does not follow it.
            let metadata = match no_follow.symlink_metadata((!path.is_empty()).then_some(path)) {
                Ok(metadata) => metadata,
                Err(err)
                    if err.kind() == ErrorKind::NotFound
                        && options.contains(RemoveOptions::IGNORE_MISSING_PATH) =>
                {
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            };

            if !metadata.is_file() && !metadata.is_symlink() {
                return Ok(());
            }
        }

        match no_follow.remove_file(path) {
            Ok(()) => Ok(()),
            Err(err)
                if err.kind() == ErrorKind::NotFound
                    && options.contains(RemoveOptions::IGNORE_MISSING_PATH) =>
            {
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Remove the directory tree at `path` recursively.
    pub fn remove_dir_all(&self, path: &RepoPath) -> Result<()> {
        self.inner.auditor.audit_components(path)?;
        Ok(self.no_follow()?.remove_dir_all(path)?)
    }

    /// Remove an empty directory at `path`.
    pub fn remove_dir(&self, path: &RepoPath) -> Result<()> {
        self.inner.auditor.audit_components(path)?;
        Ok(self.no_follow()?.remove_dir(path)?)
    }

    /// Rewrite over a symlink that already exists.
    ///
    /// Care is taken to not accidentally write _through_ the symlink.
    pub fn rewrite_symlink(
        &self,
        path: &RepoPath,
        data: blob::Blob,
        flag: UpdateFlag,
    ) -> Result<usize> {
        if !cfg!(unix) {
            // unix supports O_NOFOLLOW when opening. For Windows, just remove the file first.
            self.inner.auditor.audit_components(path)?;
            self.remove_leaf(
                path,
                RemoveOptions::IGNORE_MISSING_PATH | RemoveOptions::IGNORE_NON_FILE_OR_SYMLINK,
            )?;
        }
        self.write(path, data, flag)
    }

    // Reads file content
    pub fn read(&self, path: &RepoPath) -> Result<Bytes> {
        Ok(self.read_with_metadata(path)?.0)
    }

    // Reads file content and metadata
    pub fn read_with_metadata(&self, path: &RepoPath) -> Result<(Bytes, LiteMetadata)> {
        self.inner.auditor.audit_components(path)?;
        // This is not an atomic snapshot: the path can change between the
        // metadata query and the content read. NoFollowRoot still prevents a
        // racy symlink replacement from being followed during the read.
        let metadata = self.metadata(path)?;
        let content = if metadata.is_symlink() {
            match self.no_follow()?.read_link(path)?.to_str() {
                Some(p) => {
                    let p = if cfg!(windows) {
                        p.replace('\\', "/")
                    } else {
                        p.to_owned()
                    };
                    p.as_bytes().to_vec()
                }
                None => bail!("invalid path during vfs::read {:?}", self.join(path)),
            }
        } else {
            let mut file = self.no_follow()?.open_file(path, OpenFlags::READ, 0)?;
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            content
        };
        Ok((content.into(), metadata))
    }

    /// Converts a list of file symlinks into potentially directory symlinks by
    /// checking the final target of that symlink, and converting it into a
    /// directory one if the final target is a directory.
    #[cfg(windows)]
    pub fn reconcile_symlinks(&self, paths: &[&types::RepoPath]) -> Result<()> {
        for p in paths {
            let path = RepoPath::from_str(p.as_str())?;
            if is_final_symlink_target_dir(self.join(path))? {
                let (contents, _) = self.read_with_metadata(&path)?;
                let target = PathBuf::from(String::from_utf8(contents.into_vec())?);
                let target = util::path::replace_slash_with_backslash(&target);
                self.no_follow()?
                    .remove_file(path)
                    .context("Unable to remove symlink")?;
                self.no_follow()?
                    .write_symlink(path, &target)
                    .context("Unable to write directory symlink")?;
            }
        }
        Ok(())
    }

    pub fn supports_symlinks(&self) -> bool {
        *self
            .inner
            .supports_symlinks
            .get_or_init(|| detect_symlink_support(self.root()))
    }

    pub fn set_supports_symlinks(&self, value: bool) {
        let _ = self.inner.supports_symlinks.set(value);
    }

    pub fn supports_executables(&self) -> bool {
        self.inner.supports_executables
    }
}

#[cfg(windows)]
fn is_final_symlink_target_dir(mut path: std::path::PathBuf) -> Result<bool> {
    use std::fs;

    use anyhow::Context;
    // On Linux the usual limit for symlinks depth is 40, and symlinks stop
    // being followed after that point:
    // https://elixir.bootlin.com/linux/v6.5-rc7/source/include/linux/namei.h#L13
    // Let's keep a similar limit for Windows
    let mut rem_links = 40;
    let mut metadata = match fs::symlink_metadata(path.clone()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // The symlink file does not exist. This can happen when writes
            // failed earlier. There should be errors about those writes
            // already. Don't report a different (less readable) error.
            return Ok(false);
        }
        v => v?,
    };
    while metadata.is_symlink() && rem_links > 0 {
        rem_links -= 1;
        let target = fs::read_link(path.clone())?;
        path = path
            .parent()
            .context("unable to determine parent directory for path when resolving symlink")?
            .to_owned();
        path.push(target);
        if !path.exists() {
            // If final target doesn't exist report it as a regular file
            return Ok(false);
        }
        metadata = fs::symlink_metadata(path.clone())?;
    }
    Ok(metadata.is_dir())
}

#[cfg(unix)]
#[cfg(test)]
mod unix_tests {
    use std::fs;

    use blob::Blob;

    use super::*;

    #[test]
    fn test_symlink_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a").unwrap();
        vfs.write(path, Blob::from_static(b"abc"), UpdateFlag::Symlink)
            .unwrap();
        vfs.write(path, Blob::from_static(&[1, 2, 3]), UpdateFlag::Regular)
            .unwrap();
        let metadata = fs::symlink_metadata(vfs.join(path)).unwrap();
        assert!(metadata.file_type().is_file())
    }

    #[test]
    fn test_ancestor_symlink_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        let dir = RepoPath::from_str("a").unwrap();
        let file = RepoPath::from_str("a/b").unwrap();

        vfs.write(dir, Blob::from_static(b"abc"), UpdateFlag::Symlink)
            .unwrap();
        vfs.write(file, Blob::from_static(&[1, 2, 3]), UpdateFlag::Regular)
            .unwrap();
        let metadata = fs::symlink_metadata(vfs.join(file)).unwrap();
        assert!(metadata.file_type().is_file())
    }

    #[test]
    fn test_write_removes_ancestor_symlink_without_following_it() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link/file").unwrap();
        vfs.write(path, Blob::from_static(b"inside"), UpdateFlag::Regular)
            .unwrap();

        assert_eq!(fs::read(outside.path().join("file")).unwrap(), b"outside");
        assert_eq!(vfs.read(path).unwrap(), b"inside");
        assert!(
            fs::symlink_metadata(tmp.path().join("link"))
                .unwrap()
                .is_dir()
        );
    }

    #[test]
    fn test_read_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link/file").unwrap();
        assert!(vfs.read(path).is_err());
    }

    #[test]
    fn test_metadata_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link/file").unwrap();
        assert!(vfs.metadata(path).is_err());
    }

    #[test]
    fn test_exists_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link/file").unwrap();
        assert!(vfs.exists(path).is_err());
    }

    #[test]
    fn test_remove_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link/file").unwrap();
        assert!(
            vfs.remove(
                path,
                RemoveOptions::IGNORE_MISSING_PATH
                    | RemoveOptions::IGNORE_NON_FILE_OR_SYMLINK
                    | RemoveOptions::PRUNE_EMPTY_PARENTS,
            )
            .is_err()
        );
        assert_eq!(fs::read(outside.path().join("file")).unwrap(), b"outside");
    }

    #[test]
    fn test_remove_keeps_empty_parent_dirs_without_prune_option() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        fs::write(tmp.path().join("a/b/file"), b"content").unwrap();

        let path = RepoPath::from_str("a/b/file").unwrap();
        vfs.remove(path, RemoveOptions::empty()).unwrap();

        assert!(!tmp.path().join("a/b/file").exists());
        assert!(tmp.path().join("a/b").is_dir());
    }

    #[test]
    fn test_remove_rejects_missing_path_without_ignore_option() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a/b/file").unwrap();

        let err = vfs.remove(path, RemoveOptions::empty()).unwrap_err();
        assert_eq!(
            err.downcast_ref::<io::Error>().map(|err| err.kind()),
            Some(ErrorKind::NotFound)
        );
    }

    #[test]
    fn test_remove_ignores_missing_path_with_ignore_option() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a/b/file").unwrap();

        vfs.remove(path, RemoveOptions::IGNORE_MISSING_PATH)
            .unwrap();
    }

    #[test]
    fn test_remove_ignores_non_file_or_symlink_with_ignore_option() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        fs::create_dir(tmp.path().join("dir")).unwrap();
        let path = RepoPath::from_str("dir").unwrap();

        vfs.remove(path, RemoveOptions::IGNORE_NON_FILE_OR_SYMLINK)
            .unwrap();

        assert!(tmp.path().join("dir").is_dir());
    }

    #[test]
    fn test_remove_prunes_empty_parent_dirs_with_prune_option() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        fs::write(tmp.path().join("a/b/file"), b"content").unwrap();
        let path = RepoPath::from_str("a/b/file").unwrap();

        vfs.remove(path, RemoveOptions::PRUNE_EMPTY_PARENTS)
            .unwrap();

        assert!(!tmp.path().join("a").exists());
    }

    #[test]
    fn test_remove_dir_all_rejects_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a/b").unwrap();

        let err = vfs.remove_dir_all(path).unwrap_err();
        assert_eq!(
            err.downcast_ref::<io::Error>().map(|err| err.kind()),
            Some(ErrorKind::NotFound)
        );
    }

    #[test]
    fn test_create_dir_all_creates_missing_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a/b").unwrap();

        vfs.create_dir_all(path, Some(0o700)).unwrap();
        vfs.create_dir_all(path, Some(0o755)).unwrap();

        assert!(tmp.path().join("a/b").is_dir());
    }

    #[test]
    fn test_create_dir_all_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("a")).unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("a/link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a/link/child").unwrap();
        assert!(vfs.create_dir_all(path, None).is_err());
        assert!(!outside.path().join("child").exists());
    }

    #[test]
    fn test_list_dir_lists_root_entries() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("file"), b"content").unwrap();
        fs::create_dir(tmp.path().join("dir")).unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        assert_eq!(
            sorted_component_names(vfs.list_dir(RepoPath::empty()).unwrap()).unwrap(),
            vec!["dir".to_string(), "file".to_string()]
        );
    }

    #[test]
    fn test_list_dir_lists_child_entries() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("parent/dir")).unwrap();
        fs::write(tmp.path().join("parent/file"), b"content").unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("parent").unwrap();

        assert_eq!(
            sorted_component_names(vfs.list_dir(path).unwrap()).unwrap(),
            vec!["dir".to_string(), "file".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_list_dir_reports_invalid_entries_per_entry() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("file"), b"content").unwrap();
        fs::write(tmp.path().join("bad\nname"), b"content").unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        let names = vfs.list_dir(RepoPath::empty()).unwrap();
        let mut valid = Vec::new();
        let mut invalid_count = 0;
        for name in names {
            match name {
                Ok(name) => valid.push(name.into_string()),
                Err(err) => {
                    assert!(err.downcast_ref::<types::path::ParseError>().is_some());
                    invalid_count += 1;
                }
            }
        }
        valid.sort();

        assert_eq!(valid, vec!["file".to_string()]);
        assert_eq!(invalid_count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_list_dir_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link").unwrap();
        assert!(vfs.list_dir(path).is_err());
    }

    #[test]
    fn test_raw_no_follow_root_bypasses_auditor_for_dot_dir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join(".sl")).unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str(".sl").unwrap();

        assert!(vfs.metadata(path).is_err());
        assert!(
            vfs.raw_no_follow_root()
                .unwrap()
                .symlink_metadata(Some(path))
                .unwrap()
                .is_dir()
        );
    }

    #[test]
    fn test_set_executable_rejects_ancestor_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("link/file").unwrap();
        assert!(vfs.set_executable(path, true).is_err());
    }

    #[test]
    fn test_symlink_read() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a").unwrap();
        vfs.write(path, Blob::from_static(b"abc"), UpdateFlag::Symlink)
            .unwrap();
        let buf = vfs.read(path).unwrap();
        assert_eq!(buf, b"abc")
    }

    #[test]
    fn test_remove_dir_removes_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        fs::create_dir(tmp.path().join("dir")).unwrap();
        let path = RepoPath::from_str("dir").unwrap();

        vfs.remove_dir(path).unwrap();

        assert!(!tmp.path().join("dir").exists());
    }

    #[test]
    fn test_remove_dir_rejects_file() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        fs::write(tmp.path().join("file"), b"content").unwrap();
        let path = RepoPath::from_str("file").unwrap();

        assert!(vfs.remove_dir(path).is_err());
        assert!(tmp.path().join("file").is_file());
    }

    #[test]
    fn test_exec_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a").unwrap();
        vfs.write(path, Blob::from_static(b"abc"), UpdateFlag::Executable)
            .unwrap();
        vfs.write(path, Blob::from_static(&[1, 2, 3]), UpdateFlag::Regular)
            .unwrap();
        let mut buf = tmp.path().to_path_buf();
        buf.push("a");
        let metadata = fs::symlink_metadata(buf).unwrap();
        assert_eq!(0, metadata.permissions().mode() & 0o111)
    }

    #[test]
    fn test_set_executable_preserves_read_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("a").unwrap();
        vfs.write(path, Blob::from_static(b"abc"), UpdateFlag::Regular)
            .unwrap();
        fs::set_permissions(vfs.join(path), fs::Permissions::from_mode(0o640)).unwrap();

        vfs.set_executable(path, true).unwrap();
        let metadata = fs::symlink_metadata(vfs.join(path)).unwrap();
        assert_eq!(0o750, metadata.permissions().mode() & 0o777);

        vfs.set_executable(path, false).unwrap();
        let metadata = fs::symlink_metadata(vfs.join(path)).unwrap();
        assert_eq!(0o640, metadata.permissions().mode() & 0o777);
    }

    #[test]
    fn test_update_mode() {
        assert_eq!(0o644, VFS::update_mode(0o644, false));
        assert_eq!(0o755, VFS::update_mode(0o755, true));

        assert_eq!(0o755, VFS::update_mode(0o644, true));
        assert_eq!(0o644, VFS::update_mode(0o755, false));
    }

    fn sorted_component_names(names: Vec<Result<PathComponentBuf>>) -> Result<Vec<String>> {
        let mut names: Vec<_> = names
            .into_iter()
            .map(|name| name.map(|name| name.into_string()))
            .collect::<Result<_>>()?;
        names.sort();
        Ok(names)
    }
}

/// Since Windows determines if a file is executable based on its extension, it doesn't support
/// marking files as executable.
fn supports_executables(_fs_type: &FsType) -> bool {
    // No Windows filesystem supports Unix-style executable permissions.
    // Previously only NTFS was explicitly handled, causing false "modified"
    // reports on other filesystems like ReFS (used by Dev Drives).
    !cfg!(windows)
}

fn detect_symlink_support(root: &Path) -> bool {
    static CHECKLINK_COUNTER: AtomicUsize = AtomicUsize::new(0);
    loop {
        let link_path = root.join(format!(
            "sl-checklink-{}-{}",
            std::process::id(),
            CHECKLINK_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match symlink_file(Path::new("target"), &link_path) {
            Ok(()) => {
                let _ = std::fs::remove_file(&link_path);
                return true;
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return false,
        }
    }
}

fn known_symlink_support(fs_type: &FsType) -> Option<bool> {
    // Keep this table in sync with sapling.fscap's symlink entries. Known
    // filesystems avoid probe files, which matters for virtual filesystems like
    // EdenFS where writes can invalidate status caches.
    match *fs_type {
        FsType::APFS
        | FsType::BTRFS
        | FsType::EDENFS
        | FsType::EXT4
        | FsType::HFS
        | FsType::NTFS
        | FsType::TMPFS
        | FsType::UFS
        | FsType::XFS
        | FsType::ZFS => Some(true),
        // Certain fs, like fuse, might or might not support symlink.
        _ => None,
    }
}

/// determines whether FS located at root is case sensitive
pub fn case_sensitive(root: &Path, fs_type: &FsType) -> Result<bool> {
    // Logic in this function is consistent with util.fscasesensitive in Python
    // For some FS we know they are case (in)sensitive, so we just return based on fs type
    // For rest of the FS we see if lstat on the upper/lower case variant differs
    match *fs_type {
        FsType::EDENFS => return Ok(cfg!(target_os = "linux")),
        FsType::BTRFS => return Ok(true),
        FsType::EXT4 => return Ok(true),
        FsType::XFS => return Ok(true),
        FsType::UFS => return Ok(true),
        FsType::TMPFS => return Ok(true),
        _ => {}
    }
    detect_case_sensitive(root)
}

fn detect_case_sensitive(root: &Path) -> Result<bool> {
    let original_lstat = root.symlink_metadata()?;
    let root_str = root.to_str().expect("Can't convert root path to string");
    let mut case_different = root_str.to_lowercase();
    if case_different == root_str {
        case_different = root_str.to_uppercase();
    }
    let case_different = PathBuf::from(case_different);
    let case_different_lstat = case_different.symlink_metadata();
    if let Ok(case_different_lstat) = case_different_lstat {
        Ok(!metadata_eq(&case_different_lstat, &original_lstat)?)
    } else {
        Ok(true)
    }
}

/// Roughly compares metadata, only for internal vfs usage
/// Do not make this fn public
fn metadata_eq(m1: &Metadata, m2: &Metadata) -> Result<bool> {
    Ok(m1.modified()? == m2.modified()?
        && m1.accessed()? == m2.accessed()?
        && m1.created()? == m2.created()?
        && m1.file_type() == m2.file_type())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::io::Write;

    use blob::Blob;

    use super::*;

    #[test]
    fn test_detect_case_sensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let case_sensitive = detect_case_sensitive(tmp.path()).unwrap();
        #[cfg(target_os = "linux")]
        assert!(case_sensitive);
        #[cfg(windows)]
        assert!(!case_sensitive);
        #[cfg(target_os = "macos")]
        assert!(!case_sensitive);
    }

    #[test]
    fn test_open_reads_file() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();
        vfs.write(path, Blob::from_static(b"content"), UpdateFlag::Regular)
            .unwrap();

        let mut file = vfs.open(path, OpenFlags::READ, 0).unwrap();
        let mut content = Vec::new();
        file.read_to_end(&mut content).unwrap();
        assert_eq!(content, b"content");
    }

    #[test]
    fn test_open_vfs_scopes_to_child_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/file"), b"content").unwrap();

        let sub = RepoPath::from_str("sub").unwrap();
        let file = RepoPath::from_str("file").unwrap();
        let sub_vfs = vfs.open_vfs(sub).unwrap();

        assert_eq!(sub_vfs.root(), tmp.path().join("sub"));
        assert_eq!(sub_vfs.read(file).unwrap().as_ref(), b"content");
    }

    #[cfg(unix)]
    #[test]
    fn test_open_vfs_rejects_symlink_root() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();

        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let link = RepoPath::from_str("link").unwrap();
        assert!(vfs.open_vfs(link).is_err());
    }

    #[test]
    fn test_open_with_atomic_replace_persists_file() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();
        vfs.write(path, Blob::from_static(b"old"), UpdateFlag::Regular)
            .unwrap();

        let mut file = vfs.open_with_atomic_replace(path, 0o600).unwrap();
        file.write_all(b"new").unwrap();
        file.persist().unwrap();

        assert_eq!(vfs.read(path).unwrap().as_ref(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn test_open_create_clears_leaf_symlink_in_destructive_vfs() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();
        std::os::unix::fs::symlink("target", vfs.join(path)).unwrap();

        let mut file = vfs
            .open(
                path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                0o600,
            )
            .unwrap();
        file.write_all(b"content").unwrap();
        drop(file);

        let metadata = fs::symlink_metadata(vfs.join(path)).unwrap();
        assert!(metadata.file_type().is_file());
        assert_eq!(vfs.read(path).unwrap().as_ref(), b"content");
    }

    #[cfg(unix)]
    #[test]
    fn test_open_truncate_resets_existing_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();
        vfs.write(path, Blob::from_static(b"old"), UpdateFlag::Executable)
            .unwrap();

        let mut file = vfs
            .open(
                path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                0o644,
            )
            .unwrap();
        file.write_all(b"new").unwrap();
        drop(file);

        assert_eq!(vfs.read(path).unwrap().as_ref(), b"new");
        assert_eq!(
            fs::metadata(vfs.join(path)).unwrap().permissions().mode() & 0o777,
            util::file::apply_umask(0o644)
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_open_truncate_preserves_existing_file_with_same_mode() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();
        vfs.write(path, Blob::from_static(b"old"), UpdateFlag::Regular)
            .unwrap();
        fs::set_permissions(
            vfs.join(path),
            fs::Permissions::from_mode(util::file::apply_umask(0o644)),
        )
        .unwrap();
        let mut old_file = fs::File::open(vfs.join(path)).unwrap();

        let mut file = vfs
            .open(
                path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                0o644,
            )
            .unwrap();
        file.write_all(b"new").unwrap();
        drop(file);

        let mut content = Vec::new();
        old_file.read_to_end(&mut content).unwrap();
        assert_eq!(content, b"new");
    }

    #[cfg(unix)]
    #[test]
    fn test_open_create_uses_umask_for_new_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();

        let mut file = vfs
            .open(
                path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                0o777,
            )
            .unwrap();
        file.write_all(b"new").unwrap();
        drop(file);

        assert_eq!(
            fs::metadata(vfs.join(path)).unwrap().permissions().mode() & 0o777,
            util::file::apply_umask(0o777)
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_open_read_does_not_clear_leaf_symlink_in_destructive_vfs() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path = RepoPath::from_str("file").unwrap();
        std::os::unix::fs::symlink("target", vfs.join(path)).unwrap();

        assert!(vfs.open(path, OpenFlags::READ, 0).is_err());
        assert!(fs::symlink_metadata(vfs.join(path)).unwrap().is_symlink());
    }

    #[test]
    fn test_conflicting_file_non_destructive() {
        let tmp = tempfile::tempdir().unwrap();

        // Use VFS::new_destructive to set up the file at "a"
        let vfs_destructive = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();
        let path_a = RepoPath::from_str("a").unwrap();
        vfs_destructive
            .write(path_a, Blob::from_static(b"content"), UpdateFlag::Regular)
            .unwrap();

        // Use VFS::new (non-destructive) to try to write "a/b"
        let vfs = VFS::new(tmp.path().to_path_buf()).unwrap();

        // Now try to write "a/b" which requires "a" to be a directory, not a file
        let path_ab = RepoPath::from_str("a/b").unwrap();
        let result = vfs.write(path_ab, Blob::from_static(b"new"), UpdateFlag::Regular);

        // Should fail with ClearConflictError
        let err = result.unwrap_err();
        let conflict_err = err.downcast_ref::<ClearConflictError>().unwrap();
        assert_eq!(conflict_err.conflict_type, ConflictType::File);
        assert!(conflict_err.conflict_path.ends_with("a"));
    }

    #[test]
    fn test_conflicting_directory_non_destructive() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a directory at "a" with a file inside
        fs::create_dir(tmp.path().join("a")).unwrap();
        fs::write(tmp.path().join("a/file"), b"content").unwrap();

        // Use VFS::new (non-destructive)
        let vfs = VFS::new(tmp.path().to_path_buf()).unwrap();

        // Now try to write a file at "a" which requires the directory to be removed
        let path_a = RepoPath::from_str("a").unwrap();
        let result = vfs.write(path_a, Blob::from_static(b"new"), UpdateFlag::Regular);

        // Should fail with ClearConflictError
        let err = result.unwrap_err();
        let conflict_err = err.downcast_ref::<ClearConflictError>().unwrap();
        assert_eq!(conflict_err.conflict_type, ConflictType::Directory);
    }

    #[test]
    fn test_conflicting_file_destructive() {
        let tmp = tempfile::tempdir().unwrap();
        let vfs = VFS::new_destructive(tmp.path().to_path_buf()).unwrap();

        // Create a file at "a"
        let path_a = RepoPath::from_str("a").unwrap();
        vfs.write(path_a, Blob::from_static(b"content"), UpdateFlag::Regular)
            .unwrap();

        // Now write "a/b" - the file at "a" should be removed
        let path_ab = RepoPath::from_str("a/b").unwrap();
        vfs.write(path_ab, Blob::from_static(b"nested"), UpdateFlag::Regular)
            .unwrap();

        // Verify the file was written
        let content = vfs.read(path_ab).unwrap();
        assert_eq!(content.as_ref(), b"nested");
    }
}
