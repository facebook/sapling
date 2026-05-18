/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::fs::Metadata;
use std::io;
use std::io::Write;
use std::mem::offset_of;
use std::mem::size_of;
use std::mem::zeroed;
use std::ops::Deref;
use std::ops::DerefMut;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::AsHandle;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::BorrowedHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::path::PathBuf;
use std::ptr;
use std::ptr::null_mut;
use std::slice;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use ntapi::ntioapi::FILE_CREATE;
use ntapi::ntioapi::FILE_DELETE_ON_CLOSE;
use ntapi::ntioapi::FILE_DIRECTORY_FILE;
use ntapi::ntioapi::FILE_DIRECTORY_INFORMATION;
use ntapi::ntioapi::FILE_NON_DIRECTORY_FILE;
use ntapi::ntioapi::FILE_OPEN;
use ntapi::ntioapi::FILE_OPEN_IF;
use ntapi::ntioapi::FILE_OPEN_REPARSE_POINT;
use ntapi::ntioapi::FILE_RENAME_INFORMATION;
use ntapi::ntioapi::FILE_SYNCHRONOUS_IO_NONALERT;
use ntapi::ntioapi::FileDirectoryInformation;
use ntapi::ntioapi::FileRenameInformation;
use ntapi::ntioapi::IO_STATUS_BLOCK;
use ntapi::ntioapi::NtCreateFile;
use ntapi::ntioapi::NtQueryDirectoryFile;
use ntapi::ntioapi::NtSetInformationFile;
use ntapi::ntrtl::RtlNtStatusToDosError;
use winapi::shared::minwindef::DWORD;
use winapi::shared::minwindef::FALSE;
use winapi::shared::ntdef::FALSE as BOOLEAN_FALSE;
use winapi::shared::ntdef::HANDLE;
use winapi::shared::ntdef::InitializeObjectAttributes;
use winapi::shared::ntdef::NTSTATUS;
use winapi::shared::ntdef::OBJ_CASE_INSENSITIVE;
use winapi::shared::ntdef::OBJECT_ATTRIBUTES;
use winapi::shared::ntdef::TRUE as BOOLEAN_TRUE;
use winapi::shared::ntdef::UNICODE_STRING;
use winapi::shared::ntstatus::STATUS_BUFFER_OVERFLOW;
use winapi::shared::ntstatus::STATUS_INFO_LENGTH_MISMATCH;
use winapi::shared::ntstatus::STATUS_NO_MORE_FILES;
use winapi::shared::ntstatus::STATUS_NO_SUCH_FILE;
use winapi::um::fileapi::BY_HANDLE_FILE_INFORMATION;
use winapi::um::fileapi::CreateFileW;
use winapi::um::fileapi::FILE_BASIC_INFO;
use winapi::um::fileapi::FILE_DISPOSITION_INFO;
use winapi::um::fileapi::GetFileInformationByHandle;
use winapi::um::fileapi::GetFinalPathNameByHandleW;
use winapi::um::fileapi::OPEN_EXISTING;
use winapi::um::fileapi::SetFileInformationByHandle;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::minwinbase::FileBasicInfo;
use winapi::um::minwinbase::FileDispositionInfo;
use winapi::um::winbase::FILE_FLAG_BACKUP_SEMANTICS;
use winapi::um::winbase::FILE_FLAG_OPEN_REPARSE_POINT;
use winapi::um::winioctl::FSCTL_GET_REPARSE_POINT;
use winapi::um::winnt::DELETE;
use winapi::um::winnt::FILE_APPEND_DATA;
use winapi::um::winnt::FILE_ATTRIBUTE_DIRECTORY;
use winapi::um::winnt::FILE_ATTRIBUTE_NORMAL;
use winapi::um::winnt::FILE_ATTRIBUTE_READONLY;
use winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT;
use winapi::um::winnt::FILE_LIST_DIRECTORY;
use winapi::um::winnt::FILE_READ_ATTRIBUTES;
use winapi::um::winnt::FILE_READ_DATA;
use winapi::um::winnt::FILE_SHARE_DELETE;
use winapi::um::winnt::FILE_SHARE_READ;
use winapi::um::winnt::FILE_SHARE_WRITE;
use winapi::um::winnt::FILE_WRITE_ATTRIBUTES;
use winapi::um::winnt::FILE_WRITE_DATA;
use winapi::um::winnt::GENERIC_READ;
use winapi::um::winnt::IO_REPARSE_TAG_SYMLINK;
use winapi::um::winnt::MAXIMUM_REPARSE_DATA_BUFFER_SIZE;
use winapi::um::winnt::SYNCHRONIZE;

use super::CheckedRelPath;
use super::LiteMetadata;
use super::OpenFlags;
use super::types;
use crate::file::retry_io;
use crate::path_error;

/// A root directory handle for no-follow filesystem operations.
///
/// The root path passed to [`NoFollowRoot::new`] may contain symlinks. Child
/// directory and file operations are opened relative to the root handle and
/// reject Windows reparse points.
pub struct NoFollowRoot {
    root: OwnedHandle,
    root_ancestor_pins: Vec<OwnedHandle>,
}

impl From<Metadata> for LiteMetadata {
    fn from(metadata: Metadata) -> Self {
        let attrs = metadata.file_attributes();
        let permissions = if attrs & FILE_ATTRIBUTE_READONLY == 0 {
            0o666
        } else {
            0o444
        };
        let mode = if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            types::symlink_mode(permissions)
        } else if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
            types::dir_mode(0o777)
        } else {
            types::file_mode(permissions)
        };
        Self {
            mode,
            size: metadata.file_size(),
            accessed: filetime_u64_to_system_time(metadata.last_access_time()),
            modified: filetime_u64_to_system_time(metadata.last_write_time()),
            ctime: filetime_u64_to_system_time(metadata.creation_time()),
            // Best effort: stable Windows `std::fs::Metadata` does not expose
            // the by-handle volume serial number, file index, or link count.
            // https://github.com/rust-lang/rust/issues/63010
            dev: 0,
            ino: 0,
            nlink: 1,
            uid: 0,
            gid: 0,
        }
    }
}

impl NoFollowRoot {
    /// Open `root` as a directory.
    ///
    /// Symlinks in `root` itself are allowed.
    pub fn new(root: &Path) -> io::Result<Self> {
        retry_io(|| Self::open_inner(root))
            .map_err(|err| path_error::build(err, path_error::OPEN_FILE, root))
    }

    /// Open an existing directory below this root as a new no-follow root.
    ///
    /// `path` must be relative and must not contain `..`. No directories are
    /// created. Parent components and the leaf must not be reparse points.
    pub fn open_root<'a, P>(&self, path: P) -> io::Result<Self>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| self.open_root_inner(path.as_path()))
            .map_err(|err| path_error::build(err, path_error::OPEN_FILE, path.as_path()))
    }

    /// Write a regular file at `path`, creating parent directories.
    ///
    /// `path` must be relative and must not contain `..`. If the leaf already
    /// exists as a regular file, it is truncated. If the leaf is a reparse
    /// point, the operation fails instead of following it. The `mode` argument
    /// is ignored on Windows. This can write through hardlinks; callers that
    /// need hardlink isolation should replace the leaf instead of updating an
    /// existing file.
    pub fn write_file<'a, P>(&self, path: P, contents: &[u8], _mode: u32) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.ensure_parent_dir(path.as_path())?;
            write_file(parent.as_raw_handle() as HANDLE, leaf, contents)
        })
        .map_err(|err| path_error::build(err, path_error::WRITE_FILE, path.as_path()))
    }

    /// Open a temporary file that atomically replaces `path` when persisted.
    ///
    /// `path` must be relative and must not contain `..`. Parent directories
    /// are created as needed. Persisting the returned file renames the
    /// temporary file over the leaf. If the leaf is a reparse point, the
    /// reparse point itself is replaced; the target is not followed. The
    /// `mode` argument is ignored on Windows.
    ///
    /// Call [`AtomicReplaceFile::persist`] to replace the target and observe
    /// rename errors. Dropping without a successful `persist` discards the
    /// temporary file on a best-effort basis.
    pub fn atomic_replace_file<'a, P>(&self, path: P, _mode: u32) -> io::Result<AtomicReplaceFile>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.ensure_parent_dir(path.as_path())?;
            AtomicReplaceFile::create(parent.into_owned()?, leaf)
        })
        .map_err(|err| path_error::build(err, path_error::CREATE_FILE, path.as_path()))
    }

    /// Open a regular file at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Parent components
    /// and the leaf must not be reparse points. `flags` controls
    /// read/write/create/truncate behavior. The `mode` argument is ignored on
    /// Windows. Writable opens can write through hardlinks; callers that need
    /// hardlink isolation should replace the leaf instead of updating an
    /// existing file.
    pub fn open_file<'a, P>(&self, path: P, flags: OpenFlags, mode: u32) -> io::Result<File>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = if flags.creates_file() {
                self.ensure_parent_dir(path.as_path())?
            } else {
                self.open_parent_dir(path.as_path())?
            };
            open_file(parent.as_raw_handle() as HANDLE, leaf, flags, mode)
        })
        .map_err(|err| path_error::build(err, path_error::OPEN_FILE, path.as_path()))
    }

    /// Create a file symlink at `path`, creating parents.
    ///
    /// `path` must be relative and must not contain `..`. Parent components are
    /// checked with no-follow handle opens before creating the link. If the
    /// leaf already exists, the operation fails and leaves it untouched.
    ///
    /// On Windows, parent traversal is handle-relative and no-follow. Ideally
    /// the link would also be created through a lower-level handle API, but
    /// that path currently requires elevated symlink privileges. Instead, the
    /// checked parent directory is pinned against deletion while the symlink is
    /// created with the standard path API, which works for developer-mode
    /// unprivileged symlink creation.
    ///
    /// Windows symlinks have separate file and directory kinds, and this method
    /// chooses the kind by checking `target` before creating the link. If
    /// `target` changes concurrently, the created symlink may have the wrong
    /// kind for the new target. This race only affects the target kind: the
    /// link path's final component is created atomically, and an existing final
    /// component is not followed or replaced.
    pub fn write_symlink<'a, P>(&self, path: P, target: &Path) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.ensure_pinned_parent_dir(path.as_path())?;
            write_symlink(parent.as_raw_handle() as HANDLE, leaf, target)
        })
        .map_err(|err| path_error::build_symlink(err, target, path.as_path()))
    }

    /// Read the target of a symlink at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Parent components
    /// must not be reparse points.
    pub fn read_link<'a, P>(&self, path: P) -> io::Result<PathBuf>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            let handle = open_reparse_leaf(parent.as_raw_handle() as HANDLE, leaf)?;
            read_link(&handle)
        })
        .map_err(|err| path_error::build(err, path_error::READ_LINK, path.as_path()))
    }

    /// Return lstat-style metadata for `path`, or for the root if `path` is
    /// `None`.
    ///
    /// If provided, `path` must be relative and must not contain `..`. Parent
    /// components must not be reparse points. A reparse-point leaf is reported
    /// as a symlink-like entry instead of following its target.
    pub fn symlink_metadata<'a, P>(&self, path: Option<P>) -> io::Result<LiteMetadata>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let Some(path) = path else {
            return retry_io(|| {
                file_information(&self.root).map(|info| lite_metadata_from_info(&info))
            })
            .map_err(|err| path_error::build(err, path_error::SYMLINK_METADATA, "."));
        };
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            let handle = open_reparse_leaf(parent.as_raw_handle() as HANDLE, leaf)?;
            file_information(&handle).map(|info| lite_metadata_from_info(&info))
        })
        .map_err(super::normalize_not_directory)
        .map_err(|err| path_error::build(err, path_error::SYMLINK_METADATA, path.as_path()))
    }

    /// Remove a file or symlink at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Missing leaves are
    /// treated as success. Directories are not removed by this method; use
    /// [`NoFollowRoot::remove_dir`] for empty directories.
    pub fn remove_file<'a, P>(&self, path: P) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| self.remove_file_inner(path.as_path()))
            .map_err(|err| path_error::build(err, path_error::REMOVE_FILE, path.as_path()))
    }

    /// Remove an empty directory at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Missing leaves are
    /// treated as success.
    pub fn remove_dir<'a, P>(&self, path: P) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| self.remove_dir_inner(path.as_path()))
            .map_err(|err| path_error::build(err, path_error::REMOVE_DIR, path.as_path()))
    }

    /// Remove a directory tree at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Missing leaves are
    /// treated as success. The target leaf must be a real directory; a reparse
    /// point leaf is rejected instead of being removed. Reparse points inside
    /// the tree are removed as links and are not followed.
    pub fn remove_dir_all<'a, P>(&self, path: P) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        // Avoid retrying the whole tree from here. If retry policy ever expands
        // beyond macOS, `remove_dir_all` should retry individual operations
        // while traversing so a late timeout does not restart a large tree.
        self.remove_dir_all_inner(path.as_path())
            .map_err(|err| path_error::build(err, path_error::REMOVE_DIR, path.as_path()))
    }

    /// Set file permissions at `path`.
    ///
    /// Windows does not use Unix mode bits, so this is a no-op beyond
    /// validating that `path` is a checked relative path.
    pub fn set_permissions<'a, P>(&self, path: P, _mode: u32) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        path.try_into().map_err(Into::into).map(|_| ())
    }

    fn open_inner(root: &Path) -> io::Result<Self> {
        let wide = path_to_wide_z(root)?;
        let handle = unsafe {
            // SAFETY: `wide` is NUL-terminated and lives for the call.
            // `CreateFileW` does not retain the pointer after returning.
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        let root = unsafe {
            // SAFETY: `CreateFileW` returned a valid owned handle.
            OwnedHandle::from_raw_handle(handle as _)
        };
        let root_ancestor_pins = pin_root_ancestors(&root)?;

        Ok(Self {
            root,
            root_ancestor_pins,
        })
    }

    fn open_root_inner(&self, path: &Path) -> io::Result<Self> {
        let mut pins = Vec::with_capacity(self.root_ancestor_pins.len() + path.iter().count());
        for pin in &self.root_ancestor_pins {
            pins.push(pin.as_handle().try_clone_to_owned()?);
        }
        pins.push(self.root.as_handle().try_clone_to_owned()?);

        let mut current = self.root.as_raw_handle() as HANDLE;
        let mut root = None;
        for component in path.iter() {
            let child = open_dir_no_follow_pinned(current, component)?;
            current = child.as_raw_handle() as HANDLE;
            if let Some(parent) = root.replace(child) {
                pins.push(parent);
            }
        }

        let root = root.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path must name a file or directory: {:?}", path),
            )
        })?;

        Ok(Self {
            root,
            root_ancestor_pins: pins,
        })
    }

    fn ensure_parent_dir<'root, 'p>(
        &'root self,
        path: &'p Path,
    ) -> io::Result<(ParentHandle<'root>, &'p OsStr)> {
        let (parent_path, leaf) = split_parent_leaf(path)?;
        let parent = parent_path.iter().try_fold(
            ParentHandle::Borrowed(self.root.as_handle()),
            |dir, component| {
                open_or_create_dir(dir.as_raw_handle(), component).map(ParentHandle::Owned)
            },
        )?;
        Ok((parent, leaf))
    }

    fn open_parent_dir<'root, 'p>(
        &'root self,
        path: &'p Path,
    ) -> io::Result<(ParentHandle<'root>, &'p OsStr)> {
        let (parent_path, leaf) = split_parent_leaf(path)?;
        let parent = parent_path.iter().try_fold(
            ParentHandle::Borrowed(self.root.as_handle()),
            |dir, component| {
                open_dir_no_follow(dir.as_raw_handle(), component).map(ParentHandle::Owned)
            },
        )?;
        Ok((parent, leaf))
    }

    pub(super) fn ensure_pinned_parent_dir<'p>(
        &self,
        path: &'p Path,
    ) -> io::Result<(PinnedDir, &'p OsStr)> {
        let (parent_path, leaf) = split_parent_leaf(path)?;
        let mut handles = Vec::with_capacity(parent_path.iter().count() + 1);
        handles.push(self.root.as_handle().try_clone_to_owned()?);
        for component in parent_path.iter() {
            let parent = handles
                .last()
                .expect("pinned directory chain is never empty")
                .as_raw_handle() as HANDLE;
            handles.push(open_or_create_dir_pinned(parent, component)?);
        }
        Ok((PinnedDir::new(handles), leaf))
    }

    fn remove_file_inner(&self, path: &Path) -> io::Result<()> {
        let (parent, leaf) = self.open_parent_dir(path)?;
        let parent = parent.as_raw_handle() as HANDLE;
        match open_child(
            parent,
            leaf,
            DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT,
            FILE_ATTRIBUTE_NORMAL,
        ) {
            Ok(handle) => {
                let attrs = file_attributes(&handle)?;
                if attrs & FILE_ATTRIBUTE_DIRECTORY != 0
                    && attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0
                {
                    return Err(io::Error::from(io::ErrorKind::IsADirectory));
                }
                remove_file_handle(parent, leaf, handle)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn remove_dir_inner(&self, path: &Path) -> io::Result<()> {
        let (parent, leaf) = self.open_parent_dir(path)?;
        match open_child(
            parent.as_raw_handle() as HANDLE,
            leaf,
            DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            FILE_ATTRIBUTE_NORMAL,
        ) {
            Ok(handle) => {
                reject_reparse_point(&handle)?;
                mark_delete(&handle)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn remove_dir_all_inner(&self, path: &Path) -> io::Result<()> {
        let (parent, leaf) = self.open_parent_dir(path)?;
        match open_dir_for_remove_all(parent.as_raw_handle() as HANDLE, leaf) {
            Ok(handle) => remove_dir_all_opened(handle),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

enum ParentHandle<'a> {
    Borrowed(BorrowedHandle<'a>),
    Owned(OwnedHandle),
}

impl ParentHandle<'_> {
    fn as_raw_handle(&self) -> HANDLE {
        match self {
            Self::Borrowed(handle) => handle.as_raw_handle() as HANDLE,
            Self::Owned(handle) => handle.as_raw_handle() as HANDLE,
        }
    }

    fn into_owned(self) -> io::Result<OwnedHandle> {
        match self {
            Self::Borrowed(handle) => handle.try_clone_to_owned(),
            Self::Owned(handle) => Ok(handle),
        }
    }
}

pub struct AtomicReplaceFile {
    file: File,
    parent: OwnedHandle,
    leaf: OsString,
    persisted: bool,
}

impl AtomicReplaceFile {
    fn create(parent: OwnedHandle, leaf: &OsStr) -> io::Result<Self> {
        let file = create_temporary_file(parent.as_raw_handle() as HANDLE)?;
        Ok(Self {
            file,
            parent,
            leaf: leaf.to_os_string(),
            persisted: false,
        })
    }

    /// Rename the temporary file to the target path.
    ///
    /// This method reports write flush and rename errors. Dropping this value
    /// without a successful `persist` discards the temporary file.
    pub fn persist(&mut self) -> io::Result<()> {
        if self.persisted {
            return Ok(());
        }

        self.file.flush()?;
        rename_by_handle(
            &self.file,
            self.parent.as_raw_handle() as HANDLE,
            &self.leaf,
        )?;
        self.persisted = true;
        Ok(())
    }
}

impl Deref for AtomicReplaceFile {
    type Target = File;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

impl DerefMut for AtomicReplaceFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.file
    }
}

impl Drop for AtomicReplaceFile {
    fn drop(&mut self) {
        if !self.persisted {
            let _ = mark_delete_file(&self.file);
        }
    }
}

/// Holds directory handles opened without `FILE_SHARE_DELETE` to prevent the
/// checked path from being deleted or replaced while the symlink is created
/// through the standard path API.
pub(super) struct PinnedDir {
    handles: Vec<OwnedHandle>,
}

impl PinnedDir {
    fn new(handles: Vec<OwnedHandle>) -> Self {
        Self { handles }
    }

    fn as_raw_handle(&self) -> HANDLE {
        self.handles
            .last()
            .expect("pinned directory chain is never empty")
            .as_raw_handle() as HANDLE
    }
}

fn split_parent_leaf(path: &Path) -> io::Result<(&Path, &OsStr)> {
    let leaf = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path must name a file or directory: {:?}", path),
        )
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    Ok((parent, leaf))
}

const ROOT_ANCESTOR_PIN_ATTEMPTS: usize = 3;

fn pin_root_ancestors(root: &OwnedHandle) -> io::Result<Vec<OwnedHandle>> {
    for _ in 0..ROOT_ANCESTOR_PIN_ATTEMPTS {
        let before = final_path_by_handle(root.as_raw_handle() as HANDLE)?;
        let pins = open_root_ancestor_pins(&before)?;
        let after = final_path_by_handle(root.as_raw_handle() as HANDLE)?;
        if before == after {
            return Ok(pins);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::Other,
        "root path changed while pinning ancestors",
    ))
}

fn open_root_ancestor_pins(root_path: &Path) -> io::Result<Vec<OwnedHandle>> {
    let mut ancestors: Vec<_> = root_path
        .ancestors()
        .skip(1)
        .filter(|path| path.file_name().is_some())
        .map(Path::to_path_buf)
        .collect();
    ancestors.reverse();

    ancestors
        .iter()
        .map(|path| open_absolute_dir_pinned(path))
        .collect()
}

fn open_absolute_dir_pinned(path: &Path) -> io::Result<OwnedHandle> {
    let wide = path_to_wide_z(path)?;
    let handle = unsafe {
        // SAFETY: `wide` is NUL-terminated and lives for the call.
        // `CreateFileW` does not retain the pointer after returning.
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    Ok(unsafe {
        // SAFETY: `CreateFileW` returned a valid owned handle.
        OwnedHandle::from_raw_handle(handle as _)
    })
}

const fn no_follow_create_options(file_type_options: u32) -> u32 {
    // Creation paths need this too: if another process creates a reparse point
    // after our initial NotFound open, NtCreateFile may otherwise follow the
    // final component and create/open its target instead.
    file_type_options | FILE_OPEN_REPARSE_POINT
}

fn open_or_create_dir(dir: HANDLE, component: &OsStr) -> io::Result<OwnedHandle> {
    match open_dir_no_follow(dir, component) {
        Ok(handle) => Ok(handle),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let handle = match open_child(
                dir,
                component,
                FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                FILE_CREATE,
                no_follow_create_options(FILE_DIRECTORY_FILE),
                FILE_ATTRIBUTE_DIRECTORY,
            ) {
                Ok(handle) => handle,
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    return open_dir_no_follow(dir, component);
                }
                Err(err) => return Err(err),
            };
            reject_reparse_point(&handle)?;
            Ok(handle)
        }
        Err(err) => Err(err),
    }
}

fn open_dir_no_follow_pinned(dir: HANDLE, component: &OsStr) -> io::Result<OwnedHandle> {
    // FILE_LIST_DIRECTORY is required (not just FILE_READ_ATTRIBUTES) so that
    // the handle participates in sharing mode enforcement. Without it, Windows
    // allows RemoveDirectory to bypass the no-FILE_SHARE_DELETE constraint.
    let handle = open_child_with_share(
        dir,
        component,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?;
    reject_reparse_point(&handle)?;
    Ok(handle)
}

fn open_or_create_dir_pinned(dir: HANDLE, component: &OsStr) -> io::Result<OwnedHandle> {
    // FILE_LIST_DIRECTORY is required (not just FILE_READ_ATTRIBUTES) so that
    // the handle participates in sharing mode enforcement. Without it, Windows
    // allows RemoveDirectory to bypass the no-FILE_SHARE_DELETE constraint.
    let pinned_access = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    let pinned_share = FILE_SHARE_READ | FILE_SHARE_WRITE;

    match open_child_with_share(
        dir,
        component,
        pinned_access,
        FILE_OPEN,
        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
        pinned_share,
    ) {
        Ok(handle) => {
            reject_reparse_point(&handle)?;
            Ok(handle)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let handle = match open_child_with_share(
                dir,
                component,
                pinned_access,
                FILE_CREATE,
                no_follow_create_options(FILE_DIRECTORY_FILE),
                FILE_ATTRIBUTE_DIRECTORY,
                pinned_share,
            ) {
                Ok(handle) => handle,
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    return open_child_with_share(
                        dir,
                        component,
                        pinned_access,
                        FILE_OPEN,
                        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
                        FILE_ATTRIBUTE_NORMAL,
                        pinned_share,
                    )
                    .and_then(|handle| {
                        reject_reparse_point(&handle)?;
                        Ok(handle)
                    });
                }
                Err(err) => return Err(err),
            };
            reject_reparse_point(&handle)?;
            Ok(handle)
        }
        Err(err) => Err(err),
    }
}

fn open_dir_no_follow(dir: HANDLE, component: &OsStr) -> io::Result<OwnedHandle> {
    open_dir_no_follow_with_share(
        dir,
        component,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}

fn open_dir_no_follow_with_share(
    dir: HANDLE,
    component: &OsStr,
    share_mode: u32,
) -> io::Result<OwnedHandle> {
    let handle = open_child_with_share(
        dir,
        component,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
        share_mode,
    )?;
    reject_reparse_point(&handle)?;
    Ok(handle)
}

fn open_dir_for_remove_all(dir: HANDLE, component: &OsStr) -> io::Result<OwnedHandle> {
    let handle = open_child_with_share(
        dir,
        component,
        DELETE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?;
    reject_reparse_point(&handle)?;
    Ok(handle)
}

fn write_file(dir: HANDLE, leaf: &OsStr, contents: &[u8]) -> io::Result<()> {
    let handle = open_or_create_file_no_follow(dir, leaf)?;

    let mut file = File::from(handle);
    file.set_len(0)?;
    file.write_all(contents)
}

fn create_temporary_file(dir: HANDLE) -> io::Result<File> {
    loop {
        let name = temporary_leaf_name();
        match open_child(
            dir,
            &name,
            DELETE | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_CREATE,
            no_follow_create_options(FILE_NON_DIRECTORY_FILE),
            FILE_ATTRIBUTE_NORMAL,
        ) {
            Ok(handle) => return Ok(File::from(handle)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
}

fn temporary_leaf_name() -> OsString {
    format!(".no-follow-atomic.{:016x}.tmp", rand::random::<u64>()).into()
}

fn temporary_leaf_name_with_hint(leaf: &OsStr) -> OsString {
    let mut name = OsString::from(".");
    name.push(leaf);
    name.push(format!(".{:016x}.deleted", rand::random::<u64>()));
    name
}

fn open_file(dir: HANDLE, leaf: &OsStr, flags: OpenFlags, _mode: u32) -> io::Result<File> {
    let handle =
        open_file_no_follow_with_flags(dir, leaf, open_flags_to_windows_access(flags), flags)?;
    let file = File::from(handle);
    if flags.contains(OpenFlags::TRUNCATE) {
        file.set_len(0)?;
    }
    Ok(file)
}

fn open_or_create_file_no_follow(dir: HANDLE, leaf: &OsStr) -> io::Result<OwnedHandle> {
    match open_file_no_follow(dir, leaf, FILE_WRITE_DATA) {
        Ok(handle) => Ok(handle),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            match open_child(
                dir,
                leaf,
                FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                FILE_CREATE,
                no_follow_create_options(FILE_NON_DIRECTORY_FILE),
                FILE_ATTRIBUTE_NORMAL,
            ) {
                Ok(handle) => Ok(handle),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    open_file_no_follow(dir, leaf, FILE_WRITE_DATA)
                }
                Err(err) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn open_file_no_follow(dir: HANDLE, leaf: &OsStr, access: u32) -> io::Result<OwnedHandle> {
    open_file_no_follow_with_flags(dir, leaf, access, OpenFlags::READ)
}

fn open_file_no_follow_with_flags(
    dir: HANDLE,
    leaf: &OsStr,
    access: u32,
    flags: OpenFlags,
) -> io::Result<OwnedHandle> {
    let handle = open_child(
        dir,
        leaf,
        access | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        open_flags_to_windows_disposition(flags),
        FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
    )?;
    reject_reparse_point(&handle)?;
    Ok(handle)
}

fn open_flags_to_windows_access(flags: OpenFlags) -> u32 {
    let mut access = 0;
    if flags.contains(OpenFlags::READ) {
        access |= FILE_READ_DATA;
    }
    if flags.contains(OpenFlags::WRITE) || flags.contains(OpenFlags::TRUNCATE) {
        access |= FILE_WRITE_DATA;
    }
    if flags.contains(OpenFlags::APPEND) {
        access |= FILE_APPEND_DATA;
    }
    if access == 0 { FILE_READ_DATA } else { access }
}

fn open_flags_to_windows_disposition(flags: OpenFlags) -> u32 {
    if flags.contains(OpenFlags::CREATE_NEW) {
        FILE_CREATE
    } else if flags.contains(OpenFlags::CREATE) {
        FILE_OPEN_IF
    } else {
        FILE_OPEN
    }
}

fn open_reparse_leaf(dir: HANDLE, leaf: &OsStr) -> io::Result<OwnedHandle> {
    open_child(
        dir,
        leaf,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
    )
}

fn remove_dir_all_opened(dir: OwnedHandle) -> io::Result<()> {
    let mut stack = vec![PendingDirRemove {
        dir,
        entered: false,
    }];
    // Keep directory enumeration separate from deletion. Mutating a directory
    // while iterating it has platform-sensitive behavior, and the pending stack
    // must own child directories until their postorder delete. Reusing this
    // scratch list avoids one Vec allocation per directory.
    let mut entries = Vec::new();

    while let Some(mut pending) = stack.pop() {
        if pending.entered {
            mark_delete(&pending.dir)?;
            continue;
        }

        entries.clear();
        read_dir_entry_names(&pending.dir, |entry| {
            entries.push(entry);
            Ok(())
        })?;
        let parent = pending.dir.as_raw_handle() as HANDLE;
        pending.entered = true;
        stack.push(pending);

        for entry in entries.drain(..) {
            remove_child_all(parent, &entry, &mut stack)?;
        }
    }

    Ok(())
}

struct PendingDirRemove {
    dir: OwnedHandle,
    entered: bool,
}

fn remove_child_all(
    dir: HANDLE,
    leaf: &OsStr,
    stack: &mut Vec<PendingDirRemove>,
) -> io::Result<()> {
    let handle = match open_child(
        dir,
        leaf,
        DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
    ) {
        Ok(handle) => handle,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let attrs = file_attributes(&handle)?;
    if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 && attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        // The first handle is enough to classify the leaf without following a
        // reparse point, but directory traversal also needs FILE_LIST_DIRECTORY
        // and a no-FILE_SHARE_DELETE handle to pin the child while we enumerate
        // it. Reopen with those stronger semantics rather than using the first
        // handle for traversal. If the entry is replaced between the two opens,
        // `open_dir_for_remove_all` rechecks FILE_OPEN_REPARSE_POINT and
        // rejects symlinks/reparse points instead of following them.
        drop(handle);
        return match open_dir_for_remove_all(dir, leaf) {
            Ok(handle) => {
                stack.push(PendingDirRemove {
                    dir: handle,
                    entered: false,
                });
                Ok(())
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        };
    }
    mark_delete(&handle)
}

fn remove_file_handle(dir: HANDLE, leaf: &OsStr, handle: OwnedHandle) -> io::Result<()> {
    // Match the Windows workaround in `crate::path::remove_file`: rename before
    // deleting so the original path can be recreated even if the file's storage
    // remains alive because another process has it open or mapped. Keep the
    // operation handle-relative so the no-follow parent guarantee still holds.
    let temp = rename_to_temporary_leaf(dir, leaf, handle.as_raw_handle() as HANDLE)?;
    match mark_delete(&handle) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            drop(handle);
            delete_on_close(dir, &temp).or(Err(err))
        }
        Err(err) => Err(err),
    }
}

fn rename_to_temporary_leaf(dir: HANDLE, leaf: &OsStr, handle: HANDLE) -> io::Result<OsString> {
    loop {
        let temp = temporary_leaf_name_with_hint(leaf);
        match rename_handle(handle, dir, &temp, false) {
            Ok(()) => return Ok(temp),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
}

fn delete_on_close(dir: HANDLE, leaf: &OsStr) -> io::Result<()> {
    match open_child(
        dir,
        leaf,
        DELETE | SYNCHRONIZE,
        FILE_OPEN,
        FILE_OPEN_REPARSE_POINT | FILE_DELETE_ON_CLOSE,
        FILE_ATTRIBUTE_NORMAL,
    ) {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn read_dir_entry_names<F>(dir: &OwnedHandle, mut visit: F) -> io::Result<()>
where
    F: FnMut(OsString) -> io::Result<()>,
{
    let mut restart_scan = BOOLEAN_TRUE;
    let mut stack_buffer = [0u64; 4096 / size_of::<u64>()];
    let mut heap_buffer = Vec::new();

    loop {
        let buffer = if heap_buffer.is_empty() {
            &mut stack_buffer[..]
        } else {
            &mut heap_buffer[..]
        };
        let mut io_status: IO_STATUS_BLOCK = unsafe { zeroed() };
        let status = unsafe {
            // SAFETY: `dir` is a live directory handle. `buffer` points to
            // writable storage, aligned for `FILE_DIRECTORY_INFORMATION`.
            // All other pointer arguments are either null or live stack values
            // for the duration of the call.
            NtQueryDirectoryFile(
                dir.as_raw_handle() as HANDLE,
                null_mut(),
                None,
                null_mut(),
                &mut io_status,
                buffer.as_mut_ptr() as *mut _,
                (buffer.len() * size_of::<u64>()) as u32,
                FileDirectoryInformation,
                BOOLEAN_TRUE,
                null_mut(),
                restart_scan,
            )
        };
        restart_scan = BOOLEAN_FALSE;

        if status == STATUS_NO_MORE_FILES || status == STATUS_NO_SUCH_FILE {
            return Ok(());
        }
        if status == STATUS_BUFFER_OVERFLOW || status == STATUS_INFO_LENGTH_MISMATCH {
            let new_len = buffer.len().checked_mul(2).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "directory entry is too large")
            })?;
            heap_buffer.resize(new_len, 0);
            continue;
        }
        ntstatus_to_result(status)?;

        let bytes_returned = io_status.Information;
        let name = directory_entry_name(&buffer, bytes_returned)?;
        if name.as_os_str() != OsStr::new(".") && name.as_os_str() != OsStr::new("..") {
            // The returned name is copied out of the query buffer so callers
            // can keep it after the next `NtQueryDirectoryFile` call.
            visit(name)?;
        }
    }
}

fn directory_entry_name(buffer: &[u64], bytes_returned: usize) -> io::Result<OsString> {
    let info = buffer.as_ptr() as *const FILE_DIRECTORY_INFORMATION;
    let buffer_len = buffer.len() * size_of::<u64>();
    let file_name_offset = offset_of!(FILE_DIRECTORY_INFORMATION, FileName);
    if bytes_returned > buffer_len || bytes_returned < file_name_offset {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid directory entry size",
        ));
    }

    let file_name_len = unsafe {
        // SAFETY: `buffer` was filled by a successful `NtQueryDirectoryFile`
        // call using `FileDirectoryInformation`, is aligned to at least 8
        // bytes by the `Vec<u64>` allocation, and `bytes_returned` is large
        // enough to cover the fixed fields before `FileName`.
        (*info).FileNameLength as usize
    };
    let end = file_name_offset.checked_add(file_name_len).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "directory entry name is too long",
        )
    })?;
    if end > bytes_returned || file_name_len % size_of::<u16>() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid directory entry name range",
        ));
    }

    let file_name = unsafe {
        // SAFETY: `file_name_offset` is the offset of the trailing UTF-16 name
        // field within the allocation checked above.
        (buffer.as_ptr() as *const u8).add(file_name_offset) as *const u16
    };
    let wide = unsafe {
        // SAFETY: The range was bounds-checked above and `FileNameLength` is a
        // byte count for a UTF-16 name.
        slice::from_raw_parts(file_name, file_name_len / size_of::<u16>())
    };
    Ok(OsString::from_wide(wide))
}

fn rename_by_handle(file: &File, parent: HANDLE, leaf: &OsStr) -> io::Result<()> {
    rename_handle(file.as_raw_handle() as HANDLE, parent, leaf, true)
}

fn rename_handle(handle: HANDLE, parent: HANDLE, leaf: &OsStr, replace: bool) -> io::Result<()> {
    let mut name_len = 0usize;
    for unit in leaf.encode_wide() {
        if unit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path component contains a NUL byte: {:?}", leaf),
            ));
        }
        name_len = name_len.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path component is too long: {:?}", leaf),
            )
        })?;
    }

    let name_bytes = name_len
        .checked_mul(size_of::<u16>())
        .and_then(|len| u32::try_from(len).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path component is too long: {:?}", leaf),
            )
        })?;
    let name_offset = offset_of!(FILE_RENAME_INFORMATION, FileName);
    let total_len = name_offset
        .checked_add(name_bytes as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rename target is too long"))?;
    let total_len_u32 = u32::try_from(total_len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "rename target is too long"))?;
    let mut buffer = vec![0u64; total_len.div_ceil(size_of::<u64>())];
    let info = buffer.as_mut_ptr() as *mut FILE_RENAME_INFORMATION;
    let file_name = unsafe {
        // SAFETY: `name_offset` is within `buffer`, which was allocated from
        // the exact variable-sized length above.
        (buffer.as_mut_ptr() as *mut u8).add(name_offset) as *mut u16
    };

    unsafe {
        // SAFETY: `buffer` is aligned to at least 8 bytes and large enough for
        // the fixed `FILE_RENAME_INFORMATION` fields plus the UTF-16 leaf
        // name. The kernel copies the name during the call and does not retain
        // the pointer.
        (*info).ReplaceIfExists = if replace { BOOLEAN_TRUE } else { BOOLEAN_FALSE };
        (*info).RootDirectory = parent;
        (*info).FileNameLength = name_bytes;
        for (index, unit) in leaf.encode_wide().enumerate() {
            ptr::write(file_name.add(index), unit);
        }
    }

    let mut io_status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let status = unsafe {
        // SAFETY: `handle` is live and was opened with DELETE access. `info`
        // points to a valid variable-sized `FILE_RENAME_INFORMATION` buffer.
        NtSetInformationFile(
            handle,
            &mut io_status,
            info as *mut _,
            total_len_u32,
            FileRenameInformation,
        )
    };
    ntstatus_to_result(status)
}

fn read_link(handle: &OwnedHandle) -> io::Result<PathBuf> {
    let mut buffer = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];
    let mut bytes_written: DWORD = 0;
    let ok = unsafe {
        // SAFETY: `handle` is live. `buffer` points to writable memory for its
        // length, and `bytes_written` points to writable memory for the call.
        DeviceIoControl(
            handle.as_raw_handle() as HANDLE,
            FSCTL_GET_REPARSE_POINT,
            null_mut(),
            0,
            buffer.as_mut_ptr() as *mut _,
            buffer.len() as DWORD,
            &mut bytes_written,
            null_mut(),
        )
    };
    if ok == FALSE {
        return Err(io::Error::last_os_error());
    }

    let bytes_written = bytes_written as usize;
    if bytes_written > buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reparse buffer byte count is too large",
        ));
    }

    parse_symlink_reparse_buffer(&buffer[..bytes_written])
}

fn parse_symlink_reparse_buffer(buffer: &[u8]) -> io::Result<PathBuf> {
    const HEADER_LEN: usize = 20;
    if buffer.len() < HEADER_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "symlink reparse buffer is too short",
        ));
    }

    let tag = u32::from_le_bytes(buffer[0..4].try_into().unwrap());
    if tag != IO_REPARSE_TAG_SYMLINK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a symlink reparse point",
        ));
    }
    let reparse_data_len = u16::from_le_bytes(buffer[4..6].try_into().unwrap()) as usize;
    let reparse_len = 8usize
        .checked_add(reparse_data_len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "reparse buffer is too long"))?;
    if reparse_len < HEADER_LEN || reparse_len > buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid symlink reparse data length",
        ));
    }
    let buffer = &buffer[..reparse_len];

    let substitute_offset = u16::from_le_bytes(buffer[8..10].try_into().unwrap()) as usize;
    let substitute_len = u16::from_le_bytes(buffer[10..12].try_into().unwrap()) as usize;
    let print_offset = u16::from_le_bytes(buffer[12..14].try_into().unwrap()) as usize;
    let print_len = u16::from_le_bytes(buffer[14..16].try_into().unwrap()) as usize;
    let (offset, len) = if print_len == 0 {
        (substitute_offset, substitute_len)
    } else {
        (print_offset, print_len)
    };
    let start = HEADER_LEN
        .checked_add(offset)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "symlink target is too long"))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "symlink target is too long"))?;
    if end > buffer.len() || len % size_of::<u16>() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid symlink reparse target range",
        ));
    }

    let wide: Vec<u16> = buffer[start..end]
        .chunks_exact(size_of::<u16>())
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

fn lite_metadata_from_info(info: &BY_HANDLE_FILE_INFORMATION) -> LiteMetadata {
    let attrs = info.dwFileAttributes;
    let permissions = if attrs & FILE_ATTRIBUTE_READONLY == 0 {
        0o666
    } else {
        0o444
    };
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    let accessed = filetime_to_system_time(
        info.ftLastAccessTime.dwHighDateTime,
        info.ftLastAccessTime.dwLowDateTime,
    );
    let modified = filetime_to_system_time(
        info.ftLastWriteTime.dwHighDateTime,
        info.ftLastWriteTime.dwLowDateTime,
    );
    let ctime = filetime_to_system_time(
        info.ftCreationTime.dwHighDateTime,
        info.ftCreationTime.dwLowDateTime,
    );
    let dev = u64::from(info.dwVolumeSerialNumber);
    let ino = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let nlink = u64::from(info.nNumberOfLinks);
    let mode = if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        types::symlink_mode(permissions)
    } else if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
        types::dir_mode(0o777)
    } else {
        types::file_mode(permissions)
    };
    LiteMetadata {
        mode,
        size,
        accessed,
        modified,
        ctime,
        dev,
        ino,
        nlink,
        uid: 0,
        gid: 0,
    }
}

fn filetime_to_system_time(high: u32, low: u32) -> SystemTime {
    filetime_u64_to_system_time((u64::from(high) << 32) | u64::from(low))
}

fn filetime_u64_to_system_time(ticks: u64) -> SystemTime {
    const WINDOWS_TICKS_PER_SEC: u64 = 10_000_000;
    const WINDOWS_TO_UNIX_EPOCH_SECS: u64 = 11_644_473_600;

    let unix_epoch_ticks = WINDOWS_TO_UNIX_EPOCH_SECS * WINDOWS_TICKS_PER_SEC;
    if ticks >= unix_epoch_ticks {
        UNIX_EPOCH + Duration::from_nanos((ticks - unix_epoch_ticks) * 100)
    } else {
        UNIX_EPOCH - Duration::from_nanos((unix_epoch_ticks - ticks) * 100)
    }
}

fn write_symlink(dir: HANDLE, leaf: &OsStr, target: &Path) -> io::Result<()> {
    // Ideally this would use a lower-level handle API, similar to `symlinkat`,
    // for the final creation step too. That currently requires elevated
    // symlink privileges on Windows, so use the standard path API as a
    // workaround for developer-mode unprivileged symlink creation. The
    // checked directory chain is held without `FILE_SHARE_DELETE`, which
    // prevents any directory in the path from being deleted, renamed, or
    // replaced while the standard path API call runs. The leaf creation
    // itself remains atomic: existing leaves fail instead of being replaced.
    let parent = final_path_by_handle(dir)?;
    let target_metadata_path = if target.is_absolute() {
        target.to_path_buf()
    } else {
        parent.join(target)
    };
    let mut link = parent;
    link.push(leaf);
    // The target can change between this metadata check and the symlink
    // creation below, so this may choose the wrong Windows symlink kind. The
    // race only affects the link type; the link leaf itself is created
    // atomically and existing leaves are not followed.
    match std::fs::symlink_metadata(target_metadata_path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            std::os::windows::fs::symlink_dir(target, link)
        }
        Ok(_) => std::os::windows::fs::symlink_file(target, link),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            std::os::windows::fs::symlink_file(target, link)
        }
        Err(err) => Err(err),
    }
}

fn open_child(
    dir: HANDLE,
    component: &OsStr,
    desired_access: u32,
    create_disposition: u32,
    create_options: u32,
    file_attributes: u32,
) -> io::Result<OwnedHandle> {
    open_child_with_share(
        dir,
        component,
        desired_access,
        create_disposition,
        create_options,
        file_attributes,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}

fn open_child_with_share(
    dir: HANDLE,
    component: &OsStr,
    desired_access: u32,
    create_disposition: u32,
    create_options: u32,
    file_attributes: u32,
    share_mode: u32,
) -> io::Result<OwnedHandle> {
    let name = UnicodeString::new(component)?;
    let mut name = name.as_unicode_string();
    let mut attrs: OBJECT_ATTRIBUTES = unsafe { zeroed() };
    unsafe {
        // SAFETY: `attrs` points to initialized writable memory. `name.Buffer`
        // points to the `UnicodeString` buffer, which lives until
        // `NtCreateFile` returns. `dir` is a live directory handle for the
        // duration of the call.
        InitializeObjectAttributes(&mut attrs, &mut name, OBJ_CASE_INSENSITIVE, dir, null_mut());
    }

    let mut handle: HANDLE = null_mut();
    let mut io_status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let status = unsafe {
        // SAFETY: all pointers refer to live stack values for the duration of
        // the call. `NtCreateFile` initializes `handle` on success and does not
        // retain the object attributes pointer after returning.
        NtCreateFile(
            &mut handle,
            desired_access,
            &mut attrs,
            &mut io_status,
            null_mut(),
            file_attributes,
            share_mode,
            create_disposition,
            create_options | FILE_SYNCHRONOUS_IO_NONALERT,
            null_mut(),
            0,
        )
    };
    ntstatus_to_result(status)?;

    Ok(unsafe {
        // SAFETY: `NtCreateFile` returned a valid owned handle.
        OwnedHandle::from_raw_handle(handle as _)
    })
}

fn reject_reparse_point(handle: &OwnedHandle) -> io::Result<()> {
    let attrs = file_attributes(handle)?;
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "path component is a reparse point",
    ))
}

fn file_attributes(handle: &OwnedHandle) -> io::Result<u32> {
    file_information(handle).map(|info| info.dwFileAttributes)
}

fn file_information(handle: &OwnedHandle) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let ok = unsafe {
        // SAFETY: `handle` is live and `info` points to writable memory.
        GetFileInformationByHandle(handle.as_raw_handle() as HANDLE, &mut info)
    };
    if ok == FALSE {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

fn mark_delete(handle: &OwnedHandle) -> io::Result<()> {
    match mark_delete_once(handle) {
        Ok(()) => Ok(()),
        Err(err) => {
            let attrs = file_attributes(handle)?;
            if attrs & FILE_ATTRIBUTE_READONLY == 0 {
                return Err(err);
            }
            clear_readonly(handle, attrs)?;
            mark_delete_once(handle)
        }
    }
}

fn mark_delete_file(file: &File) -> io::Result<()> {
    mark_delete_raw(file.as_raw_handle() as HANDLE)
}

fn mark_delete_once(handle: &OwnedHandle) -> io::Result<()> {
    mark_delete_raw(handle.as_raw_handle() as HANDLE)
}

fn mark_delete_raw(handle: HANDLE) -> io::Result<()> {
    let mut info = FILE_DISPOSITION_INFO {
        DeleteFile: BOOLEAN_TRUE,
    };
    let ok = unsafe {
        // SAFETY: `handle` is live and `info` points to a valid
        // `FILE_DISPOSITION_INFO` for the duration of the call.
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            &mut info as *mut _ as *mut _,
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == FALSE {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn clear_readonly(handle: &OwnedHandle, attrs: u32) -> io::Result<()> {
    let mut info: FILE_BASIC_INFO = unsafe { zeroed() };
    info.FileAttributes = attrs & !FILE_ATTRIBUTE_READONLY;
    if info.FileAttributes == 0 {
        info.FileAttributes = FILE_ATTRIBUTE_NORMAL;
    }
    let ok = unsafe {
        // SAFETY: `handle` is live and was opened with `FILE_WRITE_ATTRIBUTES`.
        // `info` points to a valid `FILE_BASIC_INFO` for the duration of the
        // call. Zero timestamp fields leave the existing timestamps unchanged.
        SetFileInformationByHandle(
            handle.as_raw_handle() as HANDLE,
            FileBasicInfo,
            &mut info as *mut _ as *mut _,
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if ok == FALSE {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn final_path_by_handle(handle: HANDLE) -> io::Result<PathBuf> {
    let mut stack_buffer = [0u16; 260];
    let len = final_path_by_handle_into(handle, &mut stack_buffer)?;
    if len < stack_buffer.len() {
        return Ok(PathBuf::from(OsString::from_wide(&stack_buffer[..len])));
    }

    let mut buffer =
        vec![
            0;
            len.checked_add(1)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is too long"))?
        ];
    loop {
        let len = final_path_by_handle_into(handle, &mut buffer)?;
        if len < buffer.len() {
            buffer.truncate(len);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }

        buffer.resize(
            len.checked_add(1)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is too long"))?,
            0,
        );
    }
}

fn final_path_by_handle_into(handle: HANDLE, buffer: &mut [u16]) -> io::Result<usize> {
    let len = unsafe {
        // SAFETY: `handle` is live and `buffer` points to writable memory
        // for `buffer.len()` UTF-16 code units.
        GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0)
    };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }

    usize::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path is too long"))
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains a NUL byte: {:?}", path),
        ));
    }
    Ok(wide)
}

fn ntstatus_to_result(status: NTSTATUS) -> io::Result<()> {
    if status >= 0 {
        return Ok(());
    }

    let dos_error = unsafe {
        // SAFETY: `RtlNtStatusToDosError` is pure for the given status value.
        RtlNtStatusToDosError(status)
    };
    Err(io::Error::from_raw_os_error(dos_error as i32))
}

fn path_to_wide_z(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide = wide_path(path)?;
    wide.push(0);
    Ok(wide)
}

const UNICODE_STRING_STACK_CAPACITY: usize = 260;

enum UnicodeStringBuffer {
    Stack([u16; UNICODE_STRING_STACK_CAPACITY]),
    Heap(Vec<u16>),
}

impl UnicodeStringBuffer {
    fn as_ptr(&self) -> *const u16 {
        match self {
            Self::Stack(buffer) => buffer.as_ptr(),
            Self::Heap(buffer) => buffer.as_ptr(),
        }
    }
}

struct UnicodeString {
    buffer: UnicodeStringBuffer,
    byte_len: u16,
}

impl UnicodeString {
    fn new(component: &OsStr) -> io::Result<Self> {
        let mut stack = [0u16; UNICODE_STRING_STACK_CAPACITY];
        let mut len = 0usize;
        let mut units = component.encode_wide();
        while let Some(unit) = units.next() {
            if unit == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("path component contains a NUL byte: {:?}", component),
                ));
            }
            if len < stack.len() {
                stack[len] = unit;
                len += 1;
                continue;
            }

            let mut heap = Vec::with_capacity(stack.len() + 1);
            heap.extend_from_slice(&stack);
            heap.push(unit);
            for unit in units {
                if unit == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("path component contains a NUL byte: {:?}", component),
                    ));
                }
                heap.push(unit);
            }

            let byte_len = unicode_string_byte_len(heap.len(), component)?;
            return Ok(Self {
                buffer: UnicodeStringBuffer::Heap(heap),
                byte_len,
            });
        }

        let byte_len = unicode_string_byte_len(len, component)?;
        Ok(Self {
            buffer: UnicodeStringBuffer::Stack(stack),
            byte_len,
        })
    }

    fn as_unicode_string(&self) -> UNICODE_STRING {
        UNICODE_STRING {
            Length: self.byte_len,
            MaximumLength: self.byte_len,
            Buffer: self.buffer.as_ptr() as *mut _,
        }
    }
}

fn unicode_string_byte_len(len: usize, component: &OsStr) -> io::Result<u16> {
    len.checked_mul(size_of::<u16>())
        .and_then(|len| u16::try_from(len).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path component is too long: {:?}", component),
            )
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn pinned_root_rejects_delete_and_rename() -> io::Result<()> {
        let dir = tempdir()?;
        let root_path = dir.path().join("root");
        fs::create_dir(&root_path)?;
        let root = NoFollowRoot::new(&root_path)?;

        assert!(
            fs::remove_dir(&root_path).is_err(),
            "root directory should resist deletion while NoFollowRoot is alive"
        );
        assert!(
            fs::rename(&root_path, dir.path().join("renamed")).is_err(),
            "root directory should resist rename while NoFollowRoot is alive"
        );

        drop(root);
        Ok(())
    }

    #[test]
    fn pinned_root_ancestor_rejects_rename() -> io::Result<()> {
        let dir = tempdir()?;
        let parent = dir.path().join("parent");
        let root_path = parent.join("root");
        let moved = dir.path().join("moved");
        fs::create_dir_all(&root_path)?;

        let root = NoFollowRoot::new(&root_path)?;

        assert!(
            fs::rename(&parent, &moved).is_err(),
            "root ancestor should resist rename while NoFollowRoot is alive"
        );

        drop(root);
        fs::rename(&parent, &moved)?;
        Ok(())
    }

    #[test]
    fn open_root_pins_new_root_ancestors() -> io::Result<()> {
        let dir = tempdir()?;
        let parent = dir.path().join("parent");
        let child = parent.join("child");
        let moved = dir.path().join("moved");
        fs::create_dir_all(&child)?;

        let root = NoFollowRoot::new(dir.path())?;
        let child_root = root.open_root(Path::new("parent/child"))?;
        drop(root);

        assert!(
            fs::rename(&parent, &moved).is_err(),
            "opened root ancestor should resist rename while child root is alive"
        );

        drop(child_root);
        fs::rename(&parent, &moved)?;
        Ok(())
    }

    #[test]
    fn pinned_parent_chain_rejects_delete_and_rename() -> io::Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join("foo/bar"))?;
        let root = NoFollowRoot::new(dir.path())?;

        let (pinned, _) = root.ensure_pinned_parent_dir(Path::new("foo/bar/leaf"))?;

        assert!(
            fs::remove_dir(dir.path().join("foo/bar")).is_err(),
            "pinned leaf parent should resist deletion"
        );
        assert!(
            fs::rename(dir.path().join("foo/bar"), dir.path().join("foo/moved")).is_err(),
            "pinned leaf parent should resist rename"
        );
        assert!(
            fs::remove_dir(dir.path().join("foo")).is_err(),
            "pinned ancestor should resist deletion"
        );
        assert!(
            fs::rename(dir.path().join("foo"), dir.path().join("moved")).is_err(),
            "pinned ancestor should resist rename"
        );

        drop(pinned);
        Ok(())
    }

    #[test]
    fn pinned_dir_rejects_file_replacement() -> io::Result<()> {
        let dir = tempdir()?;
        fs::create_dir(dir.path().join("parent"))?;
        let root = NoFollowRoot::new(dir.path())?;

        let (pinned, _) = root.ensure_pinned_parent_dir(Path::new("parent/leaf"))?;
        let parent = dir.path().join("parent");

        assert!(
            fs::remove_dir(&parent).is_err(),
            "pinned directory should resist deletion"
        );
        assert!(
            fs::write(&parent, b"replacement").is_err(),
            "pinned directory path should resist file replacement"
        );

        drop(pinned);
        Ok(())
    }

    #[test]
    fn pinned_dir_rejects_symlink_replacement() -> io::Result<()> {
        let dir = tempdir()?;
        fs::create_dir(dir.path().join("parent"))?;
        fs::create_dir(dir.path().join("evil"))?;
        let root = NoFollowRoot::new(dir.path())?;

        let (pinned, _) = root.ensure_pinned_parent_dir(Path::new("parent/leaf"))?;
        let parent = dir.path().join("parent");
        let evil = dir.path().join("evil");

        assert!(
            fs::remove_dir(&parent).is_err(),
            "pinned directory should resist deletion"
        );
        assert!(
            std::os::windows::fs::symlink_dir(&evil, &parent).is_err(),
            "pinned directory path should resist symlink replacement"
        );

        drop(pinned);
        Ok(())
    }
}
