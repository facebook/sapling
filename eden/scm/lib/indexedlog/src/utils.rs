/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    fs::{self, File},
    hash::Hasher,
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{self, AtomicI64},
};

use crate::errors::{IoResultExt, ResultExt};
use memmap::MmapOptions;
use minibytes::Bytes;
use twox_hash::{XxHash, XxHash32};

/// Return a read-only view of the entire file.
///
/// If `len` is `None`, detect the file length automatically.
pub fn mmap_bytes(file: &File, len: Option<u64>) -> io::Result<Bytes> {
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
    if len == 0 {
        Ok(Bytes::new())
    } else {
        Ok(Bytes::from(unsafe {
            MmapOptions::new().len(len as usize).map(&file)
        }?))
    }
}

/// Similar to [`mmap_bytes`], but accepts a [`Path`] directly so the
/// callsite does not need to open a [`File`].
///
/// Return [`crate::Result`], whcih makes it easier to use for error handling.
pub fn mmap_path(path: &Path, len: u64) -> crate::Result<Bytes> {
    if len == 0 {
        Ok(Bytes::new())
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
        Ok(Bytes::from(
            mmap_bytes(&file, Some(len)).context(path, "cannot mmap")?,
        ))
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
/// Attempt to use symlinks on unix if `SYMLINK_ATOMIC_WRITE` is set.
pub fn atomic_write(
    path: impl AsRef<Path>,
    content: impl AsRef<[u8]>,
    fsync: bool,
) -> crate::Result<()> {
    let path = path.as_ref();
    let content = content.as_ref();
    #[cfg(unix)]
    {
        // Try the symlink approach first. This makes sure the file is not
        // empty.
        //
        // In theory the non-symlink approach (open, write, rename, close)
        // should also result in a non-empty file. However, we have seen empty
        // files sometimes without OS crashes (see https://fburl.com/bky2zu9e).
        if SYMLINK_ATOMIC_WRITE.load(atomic::Ordering::SeqCst) {
            if let Ok(_) = atomic_write_symlink(path, content) {
                return Ok(());
            }
        }
    }
    atomic_write_plain(path, content, fsync)
}

/// Atomically create or replace a file with the given content.
/// Use a plain file. Do not use symlinks.
pub fn atomic_write_plain(path: &Path, content: &[u8], fsync: bool) -> crate::Result<()> {
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
        let retry_limit = if cfg!(windows) { 5u16 } else { 0 };
        let mut retry = 0;
        let persisted = loop {
            match file.persist(path) {
                Ok(f) => break f,
                Err(e) => {
                    if retry < retry_limit && e.error.kind() == io::ErrorKind::PermissionDenied {
                        // Windows - rename can fail with "Access Denied" randomly.
                        // Retry a few times.
                        tracing::info!(
                            name = "atomic_write rename failed with EPERM. Will retry.",
                            retry = retry,
                            path = AsRef::<str>::as_ref(&path.display().to_string()),
                        );
                        std::thread::sleep(std::time::Duration::from_millis(1 << retry));
                        retry += 1;
                        file = e.file;
                        continue;
                    } else {
                        return Err(crate::Error::wrap(Box::new(e), "cannot persist"));
                    }
                }
            }
        };
        if fsync {
            persisted.sync_all().context(path, "cannot fsync")?;
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

/// Atomically create or replace a symlink with hex(content).
#[cfg(unix)]
fn atomic_write_symlink(path: &Path, content: &[u8]) -> io::Result<()> {
    let encoded_content: String = {
        // Use 'content' as-is if possible. Otherwise encode it using hex() and
        // prefix with 'hex:'.
        match std::str::from_utf8(content) {
            Ok(s) if !s.starts_with("hex:") && !content.contains(&0) => s.to_string(),
            _ => format!("hex:{}", hex::encode(content)),
        }
    };
    let temp_path = loop {
        let temp_path = path.with_extension(format!(".temp{}", rand::random::<u16>()));
        match std::os::unix::fs::symlink(&encoded_content, &temp_path) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Try another temp_path.
                continue;
            }
            Err(e) => return Err(e),
            Ok(_) => break temp_path,
        }
    };
    let _ = fix_perm_symlink(&temp_path);
    match fs::rename(&temp_path, path) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Clean up: Remove the temp file.
            let _ = fs::remove_file(&temp_path);
            Err(e)
        }
    }
}

/// Read the entire file written by `atomic_write`.
///
/// The read itself is only atomic if the file was written by `atomic_write`.
/// This function handles format differences (symlink vs normal files)
/// transparently.
pub fn atomic_read(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        if let Ok(data) = atomic_read_symlink(path) {
            return Ok(data);
        }
    }
    let mut file = fs::OpenOptions::new().read(true).open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Read and decode the symlink content.
#[cfg(unix)]
fn atomic_read_symlink(path: &Path) -> io::Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let encoded_content = path.read_link()?;
    let encoded_content = encoded_content.as_os_str().as_bytes();
    if encoded_content.starts_with(b"hex:") {
        // Decode hex.
        Ok(hex::decode(&encoded_content[4..]).map_err(|_e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{:?}: cannot decode hex content {:?}",
                    path, &encoded_content,
                ),
            )
        })?)
    } else {
        Ok(encoded_content.to_vec())
    }
}

/// `uid` and `gid` to `chown` for `mkdir_p`.
/// - x (x < 0): do not chown
/// - x (x >= 0): try to chown to `x`, do nothing if failed
pub static CHOWN_UID: AtomicI64 = AtomicI64::new(-1);
pub static CHOWN_GID: AtomicI64 = AtomicI64::new(-1);

/// If set to true, prefer symlinks to normal files for atomic_write.
pub static SYMLINK_ATOMIC_WRITE: atomic::AtomicBool = atomic::AtomicBool::new(cfg!(test));

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
                        if fix_perm_path(&parent, true).is_ok() {
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
            Err(err).context(dir, "cannot mkdir")
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
    #[cfg(windows)]
    {
        let _ = (path, is_dir);
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
    #[cfg(windows)]
    {
        let _ = (file, is_dir);
    }
    Ok(())
}

/// Attempt to chmod and chown a symlink at the given path.
pub(crate) fn fix_perm_symlink(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes())?;

        // chown
        let uid = CHOWN_UID.load(atomic::Ordering::SeqCst);
        let gid = CHOWN_GID.load(atomic::Ordering::SeqCst);
        if uid >= 0 || gid >= 0 {
            unsafe { libc::lchown(path.as_ptr(), uid as libc::uid_t, gid as libc::gid_t) };
        }

        // chmod
        let mode = CHMOD_FILE.load(atomic::Ordering::SeqCst);
        if mode >= 0 {
            unsafe {
                libc::fchmodat(
                    libc::AT_FDCWD,
                    path.as_ptr(),
                    mode as _,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
        }
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
    Ok(())
}

/// Return a value that is likely changing over time.
/// This is used to detect non-append-only cases.
pub(crate) fn epoch() -> u64 {
    rand::random()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_atomic_read_write(data: &[u8]) {
        SYMLINK_ATOMIC_WRITE.store(true, atomic::Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        let fsync = false;
        atomic_write(&path, data, fsync).unwrap();
        let read = atomic_read(&path).unwrap();
        assert_eq!(data, &read[..]);
    }

    #[test]
    fn test_atomic_read_write_roundtrip() {
        for data in [
            &b""[..],
            b"hex",
            b"hex:",
            b"hex:abc",
            b"hex:hex:abc",
            b"abc",
            b"\xe4\xbd\xa0\xe5\xa5\xbd",
            b"hex:\xe4\xbd\xa0\xe5\xa5\xbd",
            b"a\0b\0c\0",
            b"hex:a\0b\0c\0",
            b"\0\0\0\0\0\0",
        ]
        .iter()
        {
            check_atomic_read_write(data);
        }
    }

    quickcheck::quickcheck! {
        fn quickcheck_atomic_read_write_roundtrip(data: Vec<u8>) -> bool {
            check_atomic_read_write(&data);
            true
        }
    }
}
