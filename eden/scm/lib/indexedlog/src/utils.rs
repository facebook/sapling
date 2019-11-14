/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    fs::{self, File},
    hash::Hasher,
    io::{self, Write},
    path::Path,
    sync::atomic::{self, AtomicI64},
};

use crate::errors::{IoResultExt, ResultExt};
use memmap::{Mmap, MmapOptions};
use twox_hash::{XxHash, XxHash32};

/// Return a read-only mmap view of the entire file, and its length.
///
/// If `len` is `None`, detect the file length automatically.
///
/// For an empty file, return (1-byte mmap, 0) instead.
///
/// The caller might want to use some kind of locking to make
/// sure the file length is at some kind of boundary.
pub fn mmap_readonly(file: &File, len: Option<u64>) -> io::Result<(Mmap, u64)> {
    let actual_len = file.metadata()?.len();
    let len = match len {
        Some(len) => {
            if len > actual_len {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "mmap length {} is greater than file size {}",
                        len, actual_len
                    ),
                ));
            } else {
                len
            }
        }
        None => actual_len,
    };
    let mmap = unsafe {
        if len == 0 {
            mmap_empty()?
        } else {
            MmapOptions::new().len(len as usize).map(&file)?
        }
    };
    Ok((mmap, len))
}

/// Return a [`Mmap`] that is expected to be empty.
pub fn mmap_empty() -> io::Result<Mmap> {
    Ok(MmapOptions::new().len(1).map_anon()?.make_read_only()?)
}

/// Similar to [`mmap_readonly`], but accepts a [`Path`] directly so the
/// callsite does not need to open a [`File`].
///
/// Return [`crate::Result`], whcih makes it easier to use for error handling.
pub fn mmap_len(path: &Path, len: u64) -> crate::Result<Mmap> {
    if len == 0 {
        mmap_empty().infallible()
    } else {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .or_else(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    // This is marked as a corruption because proper NotFound
                    // handling are on non-mmapped files. For example,
                    // - Log uses "meta" not found to detect if a log is
                    //   empty/newly created. "meta" is not mmapped. If
                    //   "meta" is missing, it might be not a corruption,
                    //   but just need to create Log in-place.
                    // - RotateLog uses "latest" to detect if it is empty/
                    //   newly created. "latest" is not mmapped. If "latest"
                    //   is missing, it might be not a corruption, but just
                    //   need to create RotateLog in-place.
                    // - Index uses std::fs::OpenOptions to create new files
                    //   on demand.
                    // So mmapped files are not used to detect "whether we
                    // should create a new empty structure, or not", the
                    // NotFound issues are most likely "data corruption".
                    Err(err).context(path, "cannot open for mmap").corruption()
                } else {
                    Err(err).context(path, "cannot open for mmap")
                }
            })?;
        mmap_readonly(&file, Some(len))
            .context(path, "cannot mmap")
            .map(|(mmap, _len)| mmap)
    }
}

/// Open a path. Usually for locking purpose.
///
/// The path is assumed to be a directory. But this function does not do extra
/// checks to make sure. If path is not a directory, this function might still
/// succeed on unix systems.
///
/// Windows does not support opening a directory. This function will create a
/// file called "lock" inside the directory and open that file instead.
pub fn open_dir(lock_path: impl AsRef<Path>) -> io::Result<File> {
    let path = lock_path.as_ref();
    #[cfg(unix)]
    {
        File::open(&path)
    }
    #[cfg(not(unix))]
    {
        use std::fs;
        let mut path = path.to_path_buf();
        path.push("lock");
        fs::OpenOptions::new().write(true).create(true).open(&path)
    }
}

#[inline]
pub fn xxhash<T: AsRef<[u8]>>(buf: T) -> u64 {
    let mut xx = XxHash::default();
    xx.write(buf.as_ref());
    xx.finish()
}

#[inline]
pub fn xxhash32<T: AsRef<[u8]>>(buf: T) -> u32 {
    let mut xx = XxHash32::default();
    xx.write(buf.as_ref());
    xx.finish() as u32
}

/// Atomically create or replace a file with the given content.
pub fn atomic_write(
    path: impl AsRef<Path>,
    content: impl AsRef<[u8]>,
    fsync: bool,
) -> crate::Result<()> {
    let path = path.as_ref();
    let content = content.as_ref();
    let result: crate::Result<_> = {
        let dir = path.parent().expect("path has a parent");
        let mut file =
            tempfile::NamedTempFile::new_in(dir).context(&dir, "cannot create tempfile")?;
        file.as_file_mut()
            .write_all(content)
            .context(&file.path(), "cannot write to tempfile")?;
        if fsync {
            file.as_file_mut()
                .sync_data()
                .context(&file.path(), "cannot fdatasync")?;
        }
        // fix_perm issues are not fatal
        let _ = fix_perm_file(file.as_file(), false);
        let file = file
            .persist(path)
            .map_err(|e| crate::Error::wrap(Box::new(e), "cannot persist"))?;
        if fsync {
            file.sync_all().context(path, "cannot fsync")?;
        }
        Ok(())
    };
    result.context(|| {
        let content_desc = if content.len() < 128 {
            format!("{:?}", content)
        } else {
            format!("<{}-byte slice>", content.len())
        };
        format!(
            "  in atomic_write(path={:?}, content={}) ",
            path, content_desc
        )
    })
}

/// `uid` and `gid` to `chown` for `mkdir_p`.
/// - x (x < 0): do not chown
/// - x (x >= 0): try to chown to `x`, do nothing if failed
pub static CHOWN_UID: AtomicI64 = AtomicI64::new(-1);
pub static CHOWN_GID: AtomicI64 = AtomicI64::new(-1);

/// Default chmod mode for directories.
/// u: rwx g:rws o:r-x
pub static CHMOD_DIR: AtomicI64 = AtomicI64::new(0o2775);

// XXX: This works around https://github.com/Stebalien/tempfile/pull/61.
/// Default chmod mode for atomic_write files.
pub static CHMOD_FILE: AtomicI64 = AtomicI64::new(0o664);

/// Similar to `fs::create_dir_all`, but also attempts to chmod and chown
/// newly created directories on Unix.
pub(crate) fn mkdir_p(dir: impl AsRef<Path>) -> crate::Result<()> {
    let dir = dir.as_ref();
    let try_mkdir_once = || -> io::Result<()> {
        fs::create_dir(dir).and_then(|_| {
            // fix_perm_path issues are not fatal
            let _ = fix_perm_path(dir, true);
            Ok(())
        })
    };
    (|| -> crate::Result<()> {
        try_mkdir_once().or_else(|err| {
            match err.kind() {
                io::ErrorKind::AlreadyExists => return Ok(()),
                io::ErrorKind::NotFound => {
                    // Try to create the parent directory first.
                    if let Some(parent) = dir.parent() {
                        mkdir_p(parent)
                            .context(|| format!("while trying to mkdir_p({:?})", dir))?;
                        return try_mkdir_once()
                            .context(&dir, "cannot mkdir after mkdir its parent");
                    }
                }
                io::ErrorKind::PermissionDenied => {
                    // Try to fix permission aggressively.
                    if let Some(parent) = dir.parent() {
                        if let Ok(_) = fix_perm_path(&parent, true) {
                            return try_mkdir_once().context(&dir, "cannot mkdir").context(|| {
                                format!(
                                    "while trying to mkdir {:?} after fix_perm {:?}",
                                    &dir, &parent
                                )
                            });
                        }
                    }
                }
                _ => (),
            }
            return Err(err).context(dir, "cannot mkdir");
        })
    })()
}

/// Attempt to chmod and chown path.
pub(crate) fn fix_perm_path(path: &Path, is_dir: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        let file = fs::OpenOptions::new().read(true).open(path)?;
        fix_perm_file(&file, is_dir)?;
    }
    Ok(())
}

/// Attempt to chmod and chown a file.
pub(crate) fn fix_perm_file(file: &File, is_dir: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        // chown
        let mut uid = CHOWN_UID.load(atomic::Ordering::SeqCst);
        let mut gid = CHOWN_GID.load(atomic::Ordering::SeqCst);
        if gid >= 0 || uid >= 0 {
            let fd = std::os::unix::io::AsRawFd::as_raw_fd(file);
            if let Ok(meta) = file.metadata() {
                if uid < 0 {
                    uid = std::os::unix::fs::MetadataExt::uid(&meta) as i64;
                }
                if gid < 0 {
                    gid = std::os::unix::fs::MetadataExt::gid(&meta) as i64;
                }
                unsafe { libc::fchown(fd, uid as libc::uid_t, gid as libc::gid_t) };
            }
        }
        // chmod
        let mode = if is_dir {
            CHMOD_DIR.load(atomic::Ordering::SeqCst)
        } else {
            CHMOD_FILE.load(atomic::Ordering::SeqCst)
        };
        if mode >= 0 {
            let perm = std::os::unix::fs::PermissionsExt::from_mode(mode as u32);
            file.set_permissions(perm)?;
        }
    }
    #[allow(unreachable_code)]
    Ok(())
}

/// Return a value that is likely changing over time.
/// This is used to detect non-append-only cases.
pub(crate) fn epoch() -> u64 {
    rand::random()
}
