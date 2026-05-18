/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsStr;
use std::fs::File;
use std::fs::Metadata;
use std::io;
use std::io::Write;
#[cfg(target_os = "linux")]
use std::mem::size_of;
use std::mem::zeroed;
use std::ops::Deref;
use std::ops::DerefMut;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::fd::BorrowedFd;
use std::os::fd::FromRawFd;
use std::os::fd::IntoRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use nix::errno::Errno;

use super::CheckedRelPath;
use super::LiteMetadata;
use super::OpenFlags;
use crate::file::retry_io;
use crate::path_error;

const DEFAULT_DIR_MODE: libc::mode_t = 0o755;
#[cfg(target_os = "linux")]
static OPENAT2_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

impl From<Metadata> for LiteMetadata {
    fn from(metadata: Metadata) -> Self {
        Self {
            mode: metadata.mode(),
            size: metadata.size(),
            accessed: system_time_from_unix(metadata.atime(), metadata.atime_nsec()),
            modified: system_time_from_unix(metadata.mtime(), metadata.mtime_nsec()),
            ctime: system_time_from_unix(metadata.ctime(), metadata.ctime_nsec()),
            dev: metadata.dev(),
            ino: metadata.ino(),
            nlink: metadata.nlink(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        }
    }
}

/// A root directory handle for no-follow filesystem operations.
///
/// The root path passed to [`NoFollowRoot::new`] may contain symlinks. All
/// subsequent operations are fd-relative to the opened root and refuse symlink
/// traversal in the operation path's parent components.
pub struct NoFollowRoot {
    root: OwnedFd,
}

impl NoFollowRoot {
    /// Open `root` as a directory.
    ///
    /// Symlinks in `root` itself are allowed.
    pub fn new(root: &Path) -> io::Result<Self> {
        let fd = retry_io(|| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
                .open(root)
        })
        .map(|file| file.into())
        .map_err(|err| path_error::build(err, path_error::OPEN_FILE, root))?;
        Ok(Self { root: fd })
    }

    /// Open an existing directory below this root as a new no-follow root.
    ///
    /// `path` must be relative and must not contain `..`. No directories are
    /// created. Symlinks in parent components or at the leaf are rejected
    /// instead of followed.
    pub fn open_root<'a, P>(&self, path: P) -> io::Result<Self>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let path = path_cstring(path.as_path())?;
            open_path(
                self.root.as_fd(),
                &path,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                0,
            )
            .map(|root| Self { root })
        })
        .map_err(|err| path_error::build(err, path_error::OPEN_FILE, path.as_path()))
    }

    /// Write a regular file at `path`, creating parent directories.
    ///
    /// `path` must be relative and must not contain `..`. Parent directories
    /// are created with mode `0o755`, subject to umask. `mode` applies to the
    /// created file and is also subject to umask. If the leaf already exists as
    /// a regular file, it is truncated. If the leaf is a symlink, the operation
    /// fails instead of following it. This can write through hardlinks; callers
    /// that need hardlink isolation should remove those paths first.
    pub fn write_file<'a, P>(&self, path: P, contents: &[u8], mode: u32) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.ensure_parent_dir(path.as_path())?;
            write_file(parent.as_fd(), &leaf, mode, contents)
        })
        .map_err(|err| path_error::build(err, path_error::WRITE_FILE, path.as_path()))
    }

    /// Open a temporary file that atomically replaces `path` when persisted.
    ///
    /// `path` must be relative and must not contain `..`. Parent directories
    /// are created with mode `0o755`, subject to umask. `mode` applies to the
    /// temporary file and is also subject to umask. Persisting the returned
    /// file renames the temporary file over the leaf. If the leaf is a symlink,
    /// the symlink itself is replaced; the symlink target is not followed.
    ///
    /// Call [`AtomicReplaceFile::persist`] to replace the target and observe
    /// rename errors. Dropping without a successful `persist` discards the
    /// temporary file on a best-effort basis.
    pub fn atomic_replace_file<'a, P>(&self, path: P, mode: u32) -> io::Result<AtomicReplaceFile>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.ensure_parent_dir(path.as_path())?;
            AtomicReplaceFile::create(parent.into_owned()?, leaf, mode)
        })
        .map_err(|err| path_error::build(err, path_error::CREATE_FILE, path.as_path()))
    }

    /// Open a regular file at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Symlinks in parent
    /// components or at the leaf are rejected instead of followed. `flags`
    /// controls read/write/create/truncate behavior, and `mode` applies when a
    /// file is created.
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
            open_file(parent.as_fd(), &leaf, flags, mode)
        })
        .map_err(|err| path_error::build(err, path_error::OPEN_FILE, path.as_path()))
    }

    /// Create a symlink at `path`, creating parents.
    ///
    /// `path` must be relative and must not contain `..`. The symlink target is
    /// stored as provided. Symlinks in parent components of `path` are
    /// rejected, but the target is not resolved or validated as a filesystem
    /// path.
    pub fn write_symlink<'a, P>(&self, path: P, target: &Path) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        (|| {
            let target = path_cstring(target)?;
            let (parent, leaf) = retry_io(|| self.ensure_parent_dir(path.as_path()))?;
            // Do not retry `symlinkat` itself: if the kernel creates the link
            // and then returns a retryable error, a second create would report
            // `AlreadyExists` even though the requested operation succeeded.
            write_symlink(parent.as_fd(), &leaf, &target)
        })()
        .map_err(|err| path_error::build_symlink(err, target, path.as_path()))
    }

    /// Read the target of a symlink at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Symlinks in parent
    /// components are rejected instead of followed.
    pub fn read_link<'a, P>(&self, path: P) -> io::Result<PathBuf>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            readlinkat(parent.as_fd(), &leaf)
        })
        .map_err(|err| path_error::build(err, path_error::READ_LINK, path.as_path()))
    }

    /// Return lstat-style metadata for `path`, or for the root if `path` is
    /// `None`.
    ///
    /// If provided, `path` must be relative and must not contain `..`.
    /// Symlinks in parent components are rejected instead of followed. A
    /// symlink leaf is reported as a symlink instead of following its target.
    pub fn symlink_metadata<'a, P>(&self, path: Option<P>) -> io::Result<LiteMetadata>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let Some(path) = path else {
            return retry_io(|| {
                fstat(self.root.as_fd()).map(|stat| lite_metadata_from_stat(&stat))
            })
            .map_err(|err| path_error::build(err, path_error::SYMLINK_METADATA, "."));
        };
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            fstatat(parent.as_fd(), &leaf, libc::AT_SYMLINK_NOFOLLOW)
                .map(|stat| lite_metadata_from_stat(&stat))
        })
        .map_err(super::normalize_not_directory)
        .map_err(|err| path_error::build(err, path_error::SYMLINK_METADATA, path.as_path()))
    }

    /// Remove a file or symlink at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Directories are not
    /// removed by this method; use
    /// [`NoFollowRoot::remove_dir`] for empty directories.
    pub fn remove_file<'a, P>(&self, path: P) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        // `self.remove` wraps the fd-relative open and unlink in `retry_io`.
        self.remove(path, 0, path_error::REMOVE_FILE)
    }

    /// Remove an empty directory at `path`.
    ///
    /// `path` must be relative and must not contain `..`.
    pub fn remove_dir<'a, P>(&self, path: P) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        // `self.remove` wraps the fd-relative open and unlink in `retry_io`.
        self.remove(path, libc::AT_REMOVEDIR, path_error::REMOVE_DIR)
    }

    /// Remove a directory tree at `path`.
    ///
    /// `path` must be relative and must not contain `..`. The target leaf must
    /// be a real directory; a symlink
    /// leaf is rejected instead of being removed. Symlinks inside the tree are
    /// removed as links and are not followed.
    pub fn remove_dir_all<'a, P>(&self, path: P) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        (|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            // `remove_dir_all` retries individual fd-relative operations while
            // traversing. Do not retry the whole tree from here, since a
            // timeout late in a large tree should not restart the traversal.
            remove_dir_all(parent.as_fd(), leaf)
        })()
        .map_err(|err| path_error::build(err, path_error::REMOVE_DIR, path.as_path()))
    }

    /// Set file permissions at `path`.
    ///
    /// `path` must be relative and must not contain `..`. Symlinks in parent
    /// components or at the leaf are rejected instead of followed.
    pub fn set_permissions<'a, P>(&self, path: P, mode: u32) -> io::Result<()>
    where
        P: TryInto<CheckedRelPath<'a>>,
        P::Error: Into<io::Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        retry_io(|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            set_permissions(parent.as_fd(), &leaf, mode)
        })
        .map_err(|err| path_error::build(err, path_error::SET_PERMISSIONS, path.as_path()))
    }

    fn remove(
        &self,
        path: CheckedRelPath<'_>,
        flags: libc::c_int,
        kind: &'static str,
    ) -> io::Result<()> {
        retry_io(|| {
            let (parent, leaf) = self.open_parent_dir(path.as_path())?;
            unlinkat(parent.as_fd(), &leaf, flags)
        })
        .map_err(|err| path_error::build(err, kind, path.as_path()))
    }

    fn ensure_parent_dir(&self, path: &Path) -> io::Result<(ParentFd<'_>, CString)> {
        let (parent_path, leaf) = split_parent_leaf(path)?;
        let parent = parent_path
            .iter()
            .try_fold(ParentFd::Borrowed(self.root.as_fd()), |dir, component| {
                open_or_create_dir(dir.as_fd(), component).map(ParentFd::Owned)
            })?;
        Ok((parent, leaf))
    }

    fn open_parent_dir(&self, path: &Path) -> io::Result<(ParentFd<'_>, CString)> {
        let (parent_path, leaf) = split_parent_leaf(path)?;
        let parent = parent_path
            .iter()
            .try_fold(ParentFd::Borrowed(self.root.as_fd()), |dir, component| {
                open_dir_no_follow(dir.as_fd(), component).map(ParentFd::Owned)
            })?;
        Ok((parent, leaf))
    }
}

enum ParentFd<'a> {
    Borrowed(BorrowedFd<'a>),
    Owned(OwnedFd),
}

impl ParentFd<'_> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match self {
            Self::Borrowed(fd) => *fd,
            Self::Owned(fd) => fd.as_fd(),
        }
    }

    fn into_owned(self) -> io::Result<OwnedFd> {
        match self {
            Self::Borrowed(fd) => fd.try_clone_to_owned(),
            Self::Owned(fd) => Ok(fd),
        }
    }
}

pub struct AtomicReplaceFile {
    file: File,
    parent: OwnedFd,
    temp: CString,
    leaf: CString,
    persisted: bool,
}

impl AtomicReplaceFile {
    fn create(parent: OwnedFd, leaf: CString, mode: u32) -> io::Result<Self> {
        let (file, temp) = create_temporary_file(parent.as_fd(), mode)?;
        Ok(Self {
            file,
            parent,
            temp,
            leaf,
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
        renameat(
            self.parent.as_fd(),
            &self.temp,
            self.parent.as_fd(),
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
            let _ = unlinkat(self.parent.as_fd(), &self.temp, 0);
        }
    }
}

fn leaf_cstring(name: &OsStr) -> io::Result<CString> {
    if name.as_bytes().contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path component contains a separator: {:?}", name),
        ));
    }
    component_cstring(name)
}

fn split_parent_leaf(path: &Path) -> io::Result<(&Path, CString)> {
    let leaf = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path must name a file or directory: {:?}", path),
        )
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    Ok((parent, leaf_cstring(leaf)?))
}

fn open_or_create_dir(dir: BorrowedFd<'_>, component: &OsStr) -> io::Result<OwnedFd> {
    let name = component_cstring(component)?;
    match open_dir_no_follow_cstring(dir, &name) {
        Ok(fd) => Ok(fd),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            match mkdirat(dir, &name, DEFAULT_DIR_MODE) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
            // Another writer can race between mkdirat and this open. Reopening
            // with no-follow semantics is still required; if the name was
            // replaced by a symlink, this fails instead of traversing it.
            open_dir_no_follow_cstring(dir, &name)
        }
        Err(err) => Err(err),
    }
}

fn open_dir_no_follow(dir: BorrowedFd<'_>, component: &OsStr) -> io::Result<OwnedFd> {
    let component = component_cstring(component)?;
    open_dir_no_follow_cstring(dir, &component)
}

fn open_dir_no_follow_cstring(dir: BorrowedFd<'_>, component: &CString) -> io::Result<OwnedFd> {
    open_path(
        dir,
        component,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        0,
    )
}

fn write_file(dir: BorrowedFd<'_>, leaf: &CString, mode: u32, contents: &[u8]) -> io::Result<()> {
    let fd = open_path(
        dir,
        leaf,
        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
        mode as libc::mode_t,
    )?;
    let mut file = File::from(fd);
    file.write_all(contents)
}

fn create_temporary_file(dir: BorrowedFd<'_>, mode: u32) -> io::Result<(File, CString)> {
    loop {
        let name = temporary_leaf_name()?;
        match open_path(
            dir,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
            mode as libc::mode_t,
        ) {
            Ok(fd) => return Ok((File::from(fd), name)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
}

fn temporary_leaf_name() -> io::Result<CString> {
    CString::new(format!(
        ".no-follow-atomic.{:016x}.tmp",
        rand::random::<u64>()
    ))
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "temporary file name contains a NUL byte",
        )
    })
}

fn open_file(dir: BorrowedFd<'_>, leaf: &CString, flags: OpenFlags, mode: u32) -> io::Result<File> {
    let open_flags = open_flags_to_unix(flags);
    let mode = if open_flags & libc::O_CREAT != 0 {
        mode as libc::mode_t
    } else {
        0
    };
    let fd = open_path(dir, leaf, open_flags, mode)?;
    let stat = fstat(fd.as_fd())?;
    if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
        return Err(io::Error::from(io::ErrorKind::IsADirectory));
    }
    Ok(File::from(fd))
}

fn open_flags_to_unix(flags: OpenFlags) -> libc::c_int {
    let mut result = libc::O_CLOEXEC;
    let write = flags.contains(OpenFlags::WRITE)
        || flags.contains(OpenFlags::TRUNCATE)
        || flags.contains(OpenFlags::APPEND);
    result |= match (flags.contains(OpenFlags::READ), write) {
        (true, true) => libc::O_RDWR,
        (true, false) => libc::O_RDONLY,
        (false, true) => libc::O_WRONLY,
        (false, false) => libc::O_RDONLY,
    };
    if flags.contains(OpenFlags::CREATE) {
        result |= libc::O_CREAT;
    }
    if flags.contains(OpenFlags::CREATE_NEW) {
        result |= libc::O_CREAT | libc::O_EXCL;
    }
    if flags.contains(OpenFlags::TRUNCATE) {
        result |= libc::O_TRUNC;
    }
    if flags.contains(OpenFlags::APPEND) {
        result |= libc::O_APPEND;
    }
    result
}

fn write_symlink(dir: BorrowedFd<'_>, leaf: &CString, target: &CString) -> io::Result<()> {
    symlinkat(target, dir, leaf)
}

fn remove_dir_all(dir: BorrowedFd<'_>, leaf: CString) -> io::Result<()> {
    let child = retry_io(|| open_dir_no_follow_cstring(dir, &leaf))?;
    let mut stack = vec![PendingDirRemove {
        parent: dir.try_clone_to_owned()?,
        name: leaf,
        dir: child,
        entered: false,
    }];
    // Keep directory enumeration separate from deletion. Mutating a directory
    // while iterating its stream is platform-sensitive, and the pending stack
    // needs owned names for postorder deletion. Reusing this scratch list avoids
    // one Vec allocation per directory without borrowing names past `readdir`.
    let mut names = Vec::new();

    while let Some(mut pending) = stack.pop() {
        if pending.entered {
            remove_empty_dir(pending.parent.as_fd(), &pending.name)?;
            continue;
        }

        names.clear();
        read_dir_names(pending.dir.as_fd().try_clone_to_owned()?, |name| {
            names.push(name);
            Ok(())
        })?;
        let child_parent = pending.dir.as_fd().try_clone_to_owned()?;
        pending.entered = true;
        stack.push(pending);

        for name in names.drain(..) {
            remove_child_all(child_parent.as_fd(), name, &mut stack)?;
        }
    }

    Ok(())
}

struct PendingDirRemove {
    parent: OwnedFd,
    name: CString,
    dir: OwnedFd,
    entered: bool,
}

fn remove_child_all(
    dir: BorrowedFd<'_>,
    name: CString,
    stack: &mut Vec<PendingDirRemove>,
) -> io::Result<()> {
    let stat = match retry_io(|| fstatat(dir, &name, libc::AT_SYMLINK_NOFOLLOW)) {
        Ok(stat) => stat,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
        let child = match retry_io(|| open_dir_no_follow_cstring(dir, &name)) {
            Ok(child) => child,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        stack.push(PendingDirRemove {
            parent: dir.try_clone_to_owned()?,
            name,
            dir: child,
            entered: false,
        });
        return Ok(());
    }

    match retry_io(|| unlinkat(dir, &name, 0)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn remove_empty_dir(dir: BorrowedFd<'_>, name: &CString) -> io::Result<()> {
    match retry_io(|| unlinkat(dir, name, libc::AT_REMOVEDIR)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn set_permissions(dir: BorrowedFd<'_>, leaf: &CString, mode: u32) -> io::Result<()> {
    // `open_path` enforces no-follow semantics for the leaf and parent path.
    // Apply permissions through the opened fd so chmod cannot follow a symlink.
    let fd = open_path(
        dir,
        leaf,
        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK,
        0,
    )?;
    fchmod(fd.as_fd(), mode as libc::mode_t)
}

fn open_path(
    dir: BorrowedFd<'_>,
    path: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    let fd = open_path_at(dir, path, flags, mode);

    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `open`/`openat` returned a fresh owned file descriptor on
    // success, and this path transfers that ownership into `OwnedFd`.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn open_path_at(
    dir: BorrowedFd<'_>,
    path: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> libc::c_int {
    if OPENAT2_UNAVAILABLE.load(Ordering::Relaxed) {
        return openat_no_follow(dir, path, flags, mode);
    }

    let mut how: libc::open_how = unsafe {
        // SAFETY: `open_how` is a plain C struct. Zero is a valid default for
        // all fields; the fields required by this syscall are set below.
        zeroed()
    };
    how.flags = flags as libc::__u64;
    how.mode = mode as libc::__u64;
    how.resolve = libc::RESOLVE_NO_MAGICLINKS | libc::RESOLVE_NO_SYMLINKS;
    let fd = unsafe {
        // SAFETY: `path` is a valid NUL-terminated C string. `dir` is a live
        // borrowed directory fd for the duration of the call. `how` points to a
        // valid `open_how` for the duration of the syscall, and the kernel does
        // not retain either pointer after returning.
        libc::syscall(
            libc::SYS_openat2,
            dir.as_raw_fd(),
            path.as_ptr(),
            &how,
            size_of::<libc::open_how>(),
        ) as libc::c_int
    };
    if fd >= 0 {
        return fd;
    }

    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ENOSYS) {
        OPENAT2_UNAVAILABLE.store(true, Ordering::Relaxed);
        return openat_no_follow(dir, path, flags, mode);
    }

    fd
}

#[cfg(not(target_os = "linux"))]
fn open_path_at(
    dir: BorrowedFd<'_>,
    path: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> libc::c_int {
    openat_no_follow(dir, path, flags, mode)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn openat_no_follow(
    dir: BorrowedFd<'_>,
    path: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> libc::c_int {
    // Linux and other non-Darwin Unix platforms do not have `O_NOFOLLOW_ANY`:
    // `O_NOFOLLOW` only applies to the final path component. Walk parent
    // directories one component at a time so every intermediate component is
    // also opened as an `O_NOFOLLOW` leaf.
    if path.as_bytes().is_empty() || path.as_bytes() == b".." {
        return invalid_openat_path();
    }
    if !path.as_bytes().contains(&b'/') {
        return raw_openat(dir.as_raw_fd(), path, flags | libc::O_NOFOLLOW, mode);
    }

    let mut current_dir = None;
    let mut dir_fd = dir.as_raw_fd();
    let mut components = path.as_bytes().split(|byte| *byte == b'/').peekable();
    while let Some(component) = components.next() {
        if component == b"." {
            continue;
        }
        if component.is_empty() || component == b".." {
            return invalid_openat_path();
        }

        let component = CString::new(component).expect("component came from a CString");
        let is_leaf = components.peek().is_none();
        let fd = if is_leaf {
            raw_openat(dir_fd, &component, flags | libc::O_NOFOLLOW, mode)
        } else {
            raw_openat(
                dir_fd,
                &component,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if fd < 0 || is_leaf {
            return fd;
        }

        let next_dir = unsafe {
            // SAFETY: `raw_openat` returned a fresh owned fd for this
            // intermediate directory.
            OwnedFd::from_raw_fd(fd)
        };
        drop(current_dir.replace(next_dir));
        dir_fd = current_dir.as_ref().unwrap().as_raw_fd();
    }

    invalid_openat_path()
}

#[cfg(target_os = "macos")]
pub(super) fn openat_no_follow(
    dir: BorrowedFd<'_>,
    path: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> libc::c_int {
    // macOS uses `O_NOFOLLOW_ANY`, which applies to the whole relative path.
    let flags = flags | libc::O_NOFOLLOW_ANY;
    raw_openat(dir.as_raw_fd(), path, flags, mode)
}

fn raw_openat(
    dir_fd: libc::c_int,
    path: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> libc::c_int {
    // SAFETY: `path` is a valid NUL-terminated C string. `dir_fd` is live for
    // the duration of the call. `openat` does not retain either pointer or fd
    // after returning.
    unsafe { libc::openat(dir_fd, path.as_ptr(), flags, mode as libc::c_uint) }
}

struct Dir(*mut libc::DIR);

impl Drop for Dir {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: `self.0` came from a successful `fdopendir` call and is
            // owned by this wrapper.
            libc::closedir(self.0);
        }
    }
}

fn read_dir_names<F>(fd: OwnedFd, mut visit: F) -> io::Result<()>
where
    F: FnMut(CString) -> io::Result<()>,
{
    let raw_fd = fd.into_raw_fd();
    let dir = unsafe {
        // SAFETY: `raw_fd` is an owned directory fd. On success, ownership
        // transfers to the returned DIR pointer.
        libc::fdopendir(raw_fd)
    };
    if dir.is_null() {
        let err = io::Error::last_os_error();
        unsafe {
            // SAFETY: `fdopendir` failed, so ownership of `raw_fd` was not
            // transferred and must be closed here.
            libc::close(raw_fd);
        }
        return Err(err);
    }
    let dir = Dir(dir);

    loop {
        let entry = retry_io(|| {
            Errno::clear();
            let entry = unsafe {
                // SAFETY: `dir` is a live DIR pointer. The returned pointer, when
                // non-null, is valid until the next readdir call on the same DIR.
                libc::readdir(dir.0)
            };
            if entry.is_null() {
                let errno = Errno::last();
                if errno == Errno::from_raw(0) {
                    return Ok(None);
                }
                return Err(io::Error::last_os_error());
            }
            Ok(Some(entry))
        })?;
        let Some(entry) = entry else {
            return Ok(());
        };

        let name = unsafe {
            // SAFETY: POSIX dirent names are NUL-terminated within d_name.
            CStr::from_ptr((*entry).d_name.as_ptr())
        };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        // `readdir` reuses its internal entry buffer, so callers cannot safely
        // keep borrowed name references across iterations.
        visit(name.to_owned())?;
    }
}

fn invalid_openat_path() -> libc::c_int {
    Errno::EINVAL.set();
    -1
}

fn mkdirat(dir: BorrowedFd<'_>, path: &CString, mode: libc::mode_t) -> io::Result<()> {
    // SAFETY: `path` is a valid NUL-terminated C string. `dir` is live for the
    // duration of the call. `mkdirat` does not retain either value.
    cvt(unsafe { libc::mkdirat(dir.as_raw_fd(), path.as_ptr(), mode) })
}

fn symlinkat(target: &CString, dir: BorrowedFd<'_>, path: &CString) -> io::Result<()> {
    // SAFETY: both paths are valid NUL-terminated C strings. `dir` is live for
    // the duration of the call. `symlinkat` copies the target path into the new
    // symlink and does not retain pointers after returning.
    cvt(unsafe { libc::symlinkat(target.as_ptr(), dir.as_raw_fd(), path.as_ptr()) })
}

fn renameat(
    old_dir: BorrowedFd<'_>,
    old_path: &CString,
    new_dir: BorrowedFd<'_>,
    new_path: &CString,
) -> io::Result<()> {
    // SAFETY: both paths are valid NUL-terminated C strings. Both directory
    // fds are live for the duration of the call. `renameat` does not retain
    // any pointer or fd after returning.
    cvt(unsafe {
        libc::renameat(
            old_dir.as_raw_fd(),
            old_path.as_ptr(),
            new_dir.as_raw_fd(),
            new_path.as_ptr(),
        )
    })
}

fn readlinkat(dir: BorrowedFd<'_>, path: &CString) -> io::Result<PathBuf> {
    let mut stack_buffer = [0u8; 256];
    let len = readlinkat_into(dir, path, &mut stack_buffer)?;
    if len < stack_buffer.len() {
        return Ok(std::ffi::OsString::from_vec(stack_buffer[..len].to_vec()).into());
    }

    let mut buffer = vec![0u8; stack_buffer.len() * 2];
    loop {
        let len = readlinkat_into(dir, path, &mut buffer)?;
        if len < buffer.len() {
            buffer.truncate(len);
            return Ok(std::ffi::OsString::from_vec(buffer).into());
        }

        let new_len = buffer.len().checked_mul(2).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "symlink target is too long")
        })?;
        buffer.resize(new_len, 0);
    }
}

fn readlinkat_into(dir: BorrowedFd<'_>, path: &CString, buffer: &mut [u8]) -> io::Result<usize> {
    let len = unsafe {
        // SAFETY: `path` is a valid NUL-terminated C string. `dir` is live
        // for the duration of the call. `buffer` points to writable memory
        // for `buffer.len()` bytes.
        libc::readlinkat(
            dir.as_raw_fd(),
            path.as_ptr(),
            buffer.as_mut_ptr() as *mut libc::c_char,
            buffer.len(),
        )
    };
    if len < 0 {
        return Err(io::Error::last_os_error());
    }

    usize::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "symlink target is too long"))
}

fn unlinkat(dir: BorrowedFd<'_>, path: &CString, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: `path` is a valid NUL-terminated C string. `dir` is live for the
    // duration of the call. `unlinkat` does not retain either value.
    cvt(unsafe { libc::unlinkat(dir.as_raw_fd(), path.as_ptr(), flags) })
}

fn fstat(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    let mut stat = unsafe {
        // SAFETY: `stat` is a plain C struct. It is fully initialized by a
        // successful `fstat` call before being read.
        zeroed()
    };
    cvt(unsafe {
        // SAFETY: `fd` is live and `stat` points to writable memory.
        libc::fstat(fd.as_raw_fd(), &mut stat)
    })?;
    Ok(stat)
}

fn fstatat(dir: BorrowedFd<'_>, path: &CString, flags: libc::c_int) -> io::Result<libc::stat> {
    let mut stat = unsafe {
        // SAFETY: `stat` is a plain C struct. It is fully initialized by a
        // successful `fstatat` call before being read.
        zeroed()
    };
    cvt(unsafe {
        // SAFETY: `path` is a valid NUL-terminated C string. `dir` is live and
        // `stat` points to writable memory.
        libc::fstatat(dir.as_raw_fd(), path.as_ptr(), &mut stat, flags)
    })?;
    Ok(stat)
}

fn lite_metadata_from_stat(stat: &libc::stat) -> LiteMetadata {
    LiteMetadata {
        mode: stat_mode_to_u32(stat.st_mode),
        size: stat_size_to_u64(stat.st_size),
        accessed: stat_accessed_time(stat),
        modified: stat_modified_time(stat),
        ctime: stat_ctime(stat),
        dev: stat_dev_to_u64(stat.st_dev),
        ino: stat_ino_to_u64(stat.st_ino),
        nlink: stat_nlink_to_u64(stat.st_nlink),
        uid: stat_uid_to_u32(stat.st_uid),
        gid: stat_gid_to_u32(stat.st_gid),
    }
}

fn fchmod(fd: BorrowedFd<'_>, mode: libc::mode_t) -> io::Result<()> {
    // SAFETY: `fd` is live for the duration of the call.
    cvt(unsafe { libc::fchmod(fd.as_raw_fd(), mode) })
}

#[cfg(target_os = "linux")]
fn stat_mode_to_u32(mode: libc::mode_t) -> u32 {
    mode
}

#[cfg(not(target_os = "linux"))]
fn stat_mode_to_u32(mode: libc::mode_t) -> u32 {
    mode as u32
}

fn stat_size_to_u64(size: libc::off_t) -> u64 {
    u64::try_from(size).unwrap_or(0)
}

fn stat_dev_to_u64(dev: libc::dev_t) -> u64 {
    #[allow(clippy::useless_conversion)]
    u64::try_from(dev).unwrap_or(0)
}

fn stat_ino_to_u64(ino: libc::ino_t) -> u64 {
    ino
}

fn stat_nlink_to_u64(nlink: libc::nlink_t) -> u64 {
    u64::from(nlink)
}

fn stat_uid_to_u32(uid: libc::uid_t) -> u32 {
    uid
}

fn stat_gid_to_u32(gid: libc::gid_t) -> u32 {
    gid
}

fn stat_accessed_time(stat: &libc::stat) -> SystemTime {
    system_time_from_unix(stat.st_atime, stat_accessed_time_nsec(stat))
}

fn stat_accessed_time_nsec(stat: &libc::stat) -> i64 {
    stat.st_atime_nsec
}

fn stat_modified_time(stat: &libc::stat) -> SystemTime {
    system_time_from_unix(stat.st_mtime, stat_modified_time_nsec(stat))
}

fn stat_modified_time_nsec(stat: &libc::stat) -> i64 {
    stat.st_mtime_nsec
}

fn stat_ctime(stat: &libc::stat) -> SystemTime {
    system_time_from_unix(stat.st_ctime, stat_ctime_nsec(stat))
}

fn stat_ctime_nsec(stat: &libc::stat) -> i64 {
    stat.st_ctime_nsec
}

fn system_time_from_unix(sec: libc::time_t, nsec: i64) -> SystemTime {
    let nsec = u32::try_from(nsec)
        .ok()
        .filter(|nsec| *nsec < 1_000_000_000)
        .unwrap_or(0);
    if sec >= 0 {
        UNIX_EPOCH + Duration::new(u64::try_from(sec).unwrap_or(0), nsec)
    } else if nsec == 0 {
        UNIX_EPOCH - Duration::new(sec.unsigned_abs(), 0)
    } else {
        UNIX_EPOCH - Duration::new(sec.unsigned_abs() - 1, 1_000_000_000 - nsec)
    }
}

fn cvt(ret: libc::c_int) -> io::Result<()> {
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn path_cstring(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains a NUL byte: {:?}", path),
        )
    })
}

fn component_cstring(component: &OsStr) -> io::Result<CString> {
    CString::new(component.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path component contains a NUL byte: {:?}", component),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::UNIX_EPOCH;

    use super::system_time_from_unix;

    #[test]
    fn negative_unix_time_preserves_subsecond_offset() {
        assert_eq!(
            system_time_from_unix(-1, 500_000_000),
            UNIX_EPOCH - Duration::from_millis(500)
        );
        assert_eq!(
            system_time_from_unix(-2, 250_000_000),
            UNIX_EPOCH - Duration::from_millis(1750)
        );
    }
}
