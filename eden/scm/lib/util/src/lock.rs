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
            .path_context("error locking file", path.as_ref())?;
        Ok(PathLock { file })
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
}
