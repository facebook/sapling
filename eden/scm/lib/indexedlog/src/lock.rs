/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use fs2::FileExt;

use crate::errors::IoResultExt;
use crate::utils;

/// RAII style file locking.
pub struct ScopedFileLock<'a> {
    file: &'a mut File,
}

impl<'a> ScopedFileLock<'a> {
    pub fn new(file: &'a mut File, exclusive: bool) -> io::Result<Self> {
        if exclusive {
            file.lock_exclusive()?;
        } else {
            file.lock_shared()?;
        }
        Ok(ScopedFileLock { file })
    }
}

impl<'a> AsRef<File> for ScopedFileLock<'a> {
    fn as_ref(&self) -> &File {
        self.file
    }
}

impl<'a> AsMut<File> for ScopedFileLock<'a> {
    fn as_mut(&mut self) -> &mut File {
        self.file
    }
}

impl<'a> Drop for ScopedFileLock<'a> {
    fn drop(&mut self) {
        self.file.unlock().expect("unlock");
    }
}

/// Prove that a directory was locked.
pub struct ScopedDirLock {
    file: File,
    path: PathBuf,
}

/// Options for directory locking.
pub struct DirLockOptions {
    pub exclusive: bool,
    pub non_blocking: bool,
    pub file_name: &'static str,
}

/// Lock used to indicate that a reader is alive.
///
/// This crate generally depends on "append-only" for lock-free reads
/// (appending data won't invalidate existing readers' mmaps).
///
/// However, certain operations (ex. repair) aren't "append-only".
/// This reader lock is used to detect if any readers are alive so
/// non-append-only operations can know whether it's safe to go on.
pub(crate) static READER_LOCK_OPTS: DirLockOptions = DirLockOptions {
    exclusive: false,
    non_blocking: false,
    // The reader lock uses a different file name from the write lock,
    // because readers do not block normal writes (append-only + atomic
    // replace), and normal writes do not block readers.
    //
    // If this is "" (using default lock file), then active readers will
    // prevent normal writes, which is undesirable.
    file_name: "rlock",
};

impl ScopedDirLock {
    /// Lock the given directory with default options (exclusive, blocking).
    pub fn new(path: &Path) -> crate::Result<Self> {
        const DEFAULT_OPTIONS: DirLockOptions = DirLockOptions {
            exclusive: true,
            non_blocking: false,
            file_name: "",
        };
        Self::new_with_options(path, &DEFAULT_OPTIONS)
    }

    /// Lock the given directory with advanced options.
    ///
    /// - `opts.file_name`: decides the lock file name. A directory can have
    ///   multiple locks independent from one another using different `file_name`s.
    /// - `opts.non_blocking`: if true, do not wait and return an error if lock
    ///   cannot be obtained; if false, wait forever for the lock to be available.
    /// - `opts.exclusive`: if true, ensure that no other locks are present for
    ///   for the (dir, file_name); if false, allow other non-exclusive locks
    ///   to co-exist.
    pub fn new_with_options(dir: &Path, opts: &DirLockOptions) -> crate::Result<Self> {
        let (path, file) = if opts.file_name.is_empty() {
            let file = utils::open_dir(dir).context(dir, "cannot open for locking")?;
            (dir.to_path_buf(), file)
        } else {
            let path = dir.join(opts.file_name);

            // Try opening witout requiring write permission first. This allows
            // shared lock as a different user without write permission.
            let file = match fs::OpenOptions::new().read(true).open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // Create the file.
                    utils::mkdir_p(dir)?;
                    fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .open(&path)
                        .context(&path, "cannot create for locking")?
                }
                Err(e) => {
                    return Err(e).context(&path, "cannot open for locking");
                }
            };
            (path, file)
        };

        // Lock
        match (opts.exclusive, opts.non_blocking) {
            (true, false) => file.lock_exclusive(),
            (true, true) => file.try_lock_exclusive(),
            (false, false) => file.lock_shared(),
            (false, true) => file.try_lock_shared(),
        }
        .context(&path, || {
            format!(
                "cannot lock (exclusive: {}, non_blocking: {})",
                opts.exclusive, opts.non_blocking,
            )
        })?;

        let result = Self { file, path };
        Ok(result)
    }

    /// Get the path to the directory being locked.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScopedDirLock {
    fn drop(&mut self) {
        self.file.unlock().expect("unlock");
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;
    use std::thread;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_file_lock() {
        let dir = tempdir().unwrap();
        let _file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(dir.path().join("f"))
            .unwrap();

        const N: usize = 40;

        // Spawn N threads. Half read-only, half read-write.
        let threads: Vec<_> = (0..N)
            .map(|i| {
                let i = i;
                let path = dir.path().join("f");
                thread::spawn(move || {
                    let write = i % 2 == 0;
                    let mut file = OpenOptions::new()
                        .write(write)
                        .read(true)
                        .open(path)
                        .unwrap();
                    let mut lock = ScopedFileLock::new(&mut file, write).unwrap();
                    let len = lock.as_mut().seek(SeekFrom::End(0)).unwrap();
                    let ptr1 = lock.as_mut() as *const File;
                    let ptr2 = lock.as_ref() as *const File;
                    assert_eq!(ptr1, ptr2);
                    assert_eq!(len % 227, 0);
                    if write {
                        for j in 0..227 {
                            lock.as_mut().write_all(&[j]).expect("write");
                            lock.as_mut().flush().expect("flush");
                        }
                    }
                })
            })
            .collect();

        // Wait for them
        for thread in threads {
            thread.join().expect("joined");
        }

        // Verify the file still has a correct content
        let mut file = OpenOptions::new()
            .read(true)
            .open(dir.path().join("f"))
            .unwrap();
        let mut buf = [0u8; 227];
        let expected: Vec<u8> = (0..227).collect();
        for _ in 0..(N / 2) {
            file.read_exact(&mut buf).expect("read");
            assert_eq!(&buf[..], &expected[..]);
        }
    }

    #[test]
    fn test_dir_lock() {
        let dir = tempdir().unwrap();
        let _file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(dir.path().join("f"))
            .unwrap();

        const N: usize = 40;

        // Spawn N threads. Half read-only, half read-write.
        let threads: Vec<_> = (0..N)
            .map(|i| {
                let i = i;
                let path = dir.path().join("f");
                let dir_path = dir.path().to_path_buf();
                thread::spawn(move || {
                    let write = i % 2 == 0;
                    let mut _lock = ScopedDirLock::new(&dir_path).unwrap();
                    let mut file = OpenOptions::new()
                        .write(write)
                        .read(true)
                        .open(path)
                        .unwrap();
                    let len = file.seek(SeekFrom::End(0)).unwrap();
                    assert_eq!(len % 227, 0);
                    if write {
                        for j in 0..227 {
                            file.write_all(&[j]).expect("write");
                            file.flush().expect("flush");
                        }
                    }
                })
            })
            .collect();

        // Wait for them
        for thread in threads {
            thread.join().expect("joined");
        }

        // Verify the file still has a correct content
        let mut file = OpenOptions::new()
            .read(true)
            .open(dir.path().join("f"))
            .unwrap();
        let mut buf = [0u8; 227];
        let expected: Vec<u8> = (0..227).collect();
        for _ in 0..(N / 2) {
            file.read_exact(&mut buf).expect("read");
            assert_eq!(&buf[..], &expected[..]);
        }
    }

    #[test]
    fn test_dir_lock_with_options() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        let opts = DirLockOptions {
            file_name: "foo",
            exclusive: false,
            non_blocking: false,
        };

        // Multiple shared locks obtained with blocking on and off.
        let l1 = ScopedDirLock::new_with_options(path, &opts).unwrap();
        let l2 = ScopedDirLock::new_with_options(path, &opts).unwrap();

        let opts = DirLockOptions {
            non_blocking: true,
            ..opts
        };
        let l3 = ScopedDirLock::new_with_options(path, &opts).unwrap();

        // Exclusive lock cannot be obtained while shared locks are present.
        let opts = DirLockOptions {
            exclusive: true,
            ..opts
        };
        assert!(ScopedDirLock::new_with_options(path, &opts).is_err());

        // Exclusive lock can be obtained after releasing shared locks.
        drop((l1, l2, l3));
        let l4 = ScopedDirLock::new_with_options(path, &opts).unwrap();

        // Exclusive lock cannot be obtained while other locks are present.
        assert!(ScopedDirLock::new_with_options(path, &opts).is_err());

        // Exclusive lock cannot be obtained with a different file name.
        let opts = DirLockOptions {
            file_name: "bar",
            ..opts
        };
        assert!(ScopedDirLock::new_with_options(path, &opts).is_ok());

        drop(l4);
    }
}
