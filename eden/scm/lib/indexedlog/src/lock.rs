/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::IoResultExt;
use fs2::FileExt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

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

impl ScopedDirLock {
    /// Lock the given directory.
    pub fn new(path: &Path) -> crate::Result<Self> {
        let file = crate::utils::open_dir(path).context(path, "cannot open for locking")?;
        file.lock_exclusive().context(path, "cannot lock")?;
        let result = Self {
            file,
            path: path.to_path_buf(),
        };
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
    use super::*;
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::thread;
    use tempfile::tempdir;

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
}
