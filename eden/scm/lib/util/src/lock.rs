/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::error;
use std::fmt;
#[cfg(unix)]
use std::fs::Permissions;
use std::io;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use fs::File;
use fs_err as fs;
#[cfg(windows)]
use winapi::shared::winerror::ERROR_NOT_LOCKED;

use crate::errors::IOContext;
use crate::file::open;

/// RAII lock on a filesystem path.
#[derive(Debug)]
pub struct PathLock {
    file: File,
}

impl PathLock {
    /// Take an exclusive lock on `path`. The lock file will be created on
    /// demand.
    /// Waits for the lock to be freed, if it's currently locked.
    pub fn exclusive<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = open_lockfile(path.as_ref())?;
        fs2::FileExt::lock_exclusive(file.file())
            .path_context("error exclusive locking file", path.as_ref())?;
        Ok(PathLock { file })
    }

    pub fn shared<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = open_lockfile(path.as_ref())?;
        fs2::FileExt::lock_shared(file.file())
            .path_context("error shared locking file", path.as_ref())?;
        Ok(PathLock { file })
    }

    fn new(file: File) -> Self {
        PathLock { file }
    }

    pub fn unlock(&self) -> io::Result<()> {
        fs2::FileExt::unlock(self.file.file())
            .path_context("error unlocking file", self.file.path())
    }

    pub fn as_file(&self) -> &File {
        &self.file
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        if let Err(err) = self.unlock() {
            match err.raw_os_error().map(|x| x as u32) {
                // On windows, double unlock raises an error(Id 158). Ignore
                // since we're dropping the handle anyway.
                #[cfg(windows)]
                Some(ERROR_NOT_LOCKED) => {}
                _ => {
                    tracing::error!("unlock error: {}", err);
                }
            };
        }
    }
}

pub fn open_lockfile<P: AsRef<Path>>(path: P) -> io::Result<File> {
    let path_exists = path.as_ref().exists();
    let file = open(path.as_ref(), "wc").io_context("lock file")?;
    #[cfg(unix)]
    if !path_exists {
        // Set permissions, in case root is creating the lock
        let _ = file.set_permissions(Permissions::from_mode(0o666));
    }
    Ok(file)
}

pub fn try_lock_exclusive<P: AsRef<Path>>(path: P) -> io::Result<File> {
    let file = open_lockfile(path.as_ref())?;
    fs2::FileExt::try_lock_exclusive(file.file())?;
    Ok(file)
}

pub fn try_lock_shared<P: AsRef<Path>>(path: P) -> io::Result<File> {
    let file = open_lockfile(path.as_ref())?;
    fs2::FileExt::try_lock_shared(file.file())?;
    Ok(file)
}

pub struct ContentLock {
    // Contains paths needed for an advisory lock and some contents that
    // get returned when trying to access the lock while it is held.
    pub content_path: PathBuf,
    pub lock_path: PathBuf,
    pub dir_lock_path: PathBuf,
}

impl ContentLock {
    pub fn new<P: AsRef<Path>>(content_path: P) -> Self {
        let lock_path = content_path.as_ref().with_extension("lock");
        let dir_lock_path = content_path.as_ref().with_file_name(".dir_lock");
        Self {
            content_path: content_path.as_ref().to_path_buf(),
            lock_path,
            dir_lock_path,
        }
    }

    pub fn new_with_name<P: AsRef<Path>>(dir: P, name: &str) -> Self {
        let content_path = dir.as_ref().join(sanitize_lock_name(name));
        Self::new(&content_path)
    }

    // Take an exclusive lock on `path`. The lock file will be created on
    // demand. If the lock is successfully acquired, write `contents` to `content_path`.
    // If the lock file is already locked, return a ContentLockError(ContentLockError)
    // with the contents of the lock file.
    //
    //   Our locking strategy uses three files:
    //   1. An empty advisory lock file at the directory level.
    //   2. An empty advisory lock file named <name>.lock. This file is returned.
    //   3. An file named <name>. This file contains the specified contents
    //
    //  Readers and writers acquire the directory lock first. This
    //  ensures atomicity across lock acquisition and notification
    //
    pub fn try_lock(&self, contents: &[u8]) -> Result<PathLock, ContentLockError> {
        // Hold the PathLock during this function, then drop it at the end
        let _dir_lock = PathLock::exclusive(&self.dir_lock_path)?;
        let file = match try_lock_exclusive(&self.lock_path) {
            Ok(lock_file) => lock_file,
            Err(err) if err.kind() == fs2::lock_contended_error().kind() => {
                let contents = fs_err::read(&self.content_path)?;
                return Err(LockContendedError {
                    path: self.content_path.clone(),
                    contents,
                }
                .into());
            }
            Err(err) => {
                return Err(crate::errors::from_err_msg_path(
                    err,
                    "error locking lock file",
                    &self.lock_path,
                )
                .into());
            }
        };
        let mut contents_file = crate::file::open(&self.content_path, "wct")?;
        #[cfg(unix)]
        let _ = contents_file.set_permissions(Permissions::from_mode(0o666));
        contents_file
            .write_all(contents.as_ref())
            .path_context("error write lock contents", &self.content_path)?;
        Ok(PathLock::new(file))
    }

    // Checks if there is a lock on path. Returns () if there isn't one.
    // If there is already a lock on path, returns the value contained in content_path
    pub fn check_lock(&self) -> Result<(), ContentLockError> {
        if !self.dir_lock_path.try_exists()? || !self.lock_path.try_exists()? {
            return Ok(());
        }
        let _dir_lock = PathLock::shared(&self.dir_lock_path)?;
        match try_lock_shared(&self.lock_path) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == fs2::lock_contended_error().kind() => {
                let contents = fs_err::read(&self.content_path)?;
                Err(LockContendedError {
                    path: self.content_path.clone(),
                    contents,
                }
                .into())
            }
            Err(err) => Err(crate::errors::from_err_msg_path(
                err,
                "error locking lock file",
                &self.lock_path,
            )
            .into()),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ContentLockError {
    #[error(transparent)]
    Contended(#[from] LockContendedError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl ContentLockError {
    /// Test if the error is contended, aka. held by others.
    pub fn is_contended(&self) -> bool {
        matches!(self, ContentLockError::Contended(_))
    }
}

#[derive(Debug)]
pub struct LockContendedError {
    pub path: PathBuf,
    pub contents: Vec<u8>,
}
impl error::Error for LockContendedError {}

impl fmt::Display for LockContendedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lock {:?} contended", self.path)
    }
}

pub fn sanitize_lock_name(name: &str) -> String {
    // Avoid letting a caller specify "foo.lock" and accidentally
    // interfering with the underlying locking details. This is
    // mainly for compatibility during python lock transition to
    // avoid a python lock "foo.lock" accidentally colliding with
    // the rust lock file.
    name.replace('.', "_")
}

pub fn unsanitize_lock_name(name: &str) -> String {
    // Undo above sanitization
    name.replace('_', ".")
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::thread;

    use super::*;

    #[test]
    fn test_path_lock() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a");
        let (tx, rx) = channel();
        const N: usize = 50;
        let threads: Vec<_> = (0..N)
            .map(|i| {
                let path = path.clone();
                let tx = tx.clone();
                thread::spawn(move || {
                    // Write 2 values that are the same, protected by the lock.
                    let _locked = PathLock::exclusive(&path);
                    tx.send(i).unwrap();
                    tx.send(i).unwrap();
                })
            })
            .collect();

        for thread in threads {
            thread.join().expect("joined");
        }

        for _ in 0..N {
            // Read 2 values. They should be the same.
            let v1 = rx.recv().unwrap();
            let v2 = rx.recv().unwrap();
            assert_eq!(v1, v2);
        }

        Ok(())
    }

    #[test]
    fn test_pathlock_double_unlock() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a");
        let locked = PathLock::exclusive(&path)?;
        locked.unlock()?;
        let unlock_res = locked.unlock();
        #[cfg(windows)]
        assert!(unlock_res.is_err());
        #[cfg(unix)]
        assert!(unlock_res.is_ok());
        Ok(())
    }

    #[test]
    fn test_shared_lock() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a");
        let _shared_lock = PathLock::shared(&path)?;
        let _shared_lock2 = PathLock::shared(&path)?;
        let _shared_lock3 = PathLock::shared(&path)?;

        let content_lock = ContentLock::new(&path);
        let _ = content_lock.check_lock()?;
        let _ = content_lock.check_lock()?;
        let _ = content_lock.check_lock()?;

        Ok(())
    }

    #[test]
    fn test_content_lock() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a");
        let content_lock = ContentLock::new(&path);
        let _locked = content_lock.try_lock(b"foo")?;
        let contents = content_lock.check_lock().unwrap_err();
        assert!(contents.is_contended());
        match contents {
            ContentLockError::Contended(err) => {
                assert_eq!(err.path, path);
                assert_eq!(err.contents, b"foo");
            }
            _ => panic!("Expected a Contended error"),
        }

        Ok(())
    }
}
