// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    fs::{self, File},
    hash::Hasher,
    io::{self, Write},
    path::Path,
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The tempfile crate is working on adding a way to do this automatically, until then, we
            // need to do that by hand.
            // https://github.com/Stebalien/tempfile/pull/61
            let permissions = PermissionsExt::from_mode(0o664);
            file.as_file()
                .set_permissions(permissions)
                .context(&file.path(), "cannot chmod")?;
        }
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

/// Similar to `fs::create_dir_all`, but also attempts to chmod it on Unix.
pub(crate) fn mkdir_p(dir: impl AsRef<Path>) -> crate::Result<()> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir).context(dir, "cannot mkdir")?;
    #[cfg(unix)]
    {
        // u: rwx g:rws o:r-x
        let perm = std::os::unix::fs::PermissionsExt::from_mode(0o2775);
        // chmod errors are not fatal
        let _ = fs::set_permissions(dir, perm);
    }
    Ok(())
}

/// Return a value that is likely changing over time.
/// This is used to detect non-append-only cases.
pub(crate) fn epoch() -> u64 {
    rand::random()
}
