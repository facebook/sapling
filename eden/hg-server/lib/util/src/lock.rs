/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;

use fs2::FileExt;

/// RAII lock on a filesystem path.
pub struct PathLock {
    file: File,
}

impl PathLock {
    /// Take an exclusive lock on `path`. The lock file will be created on
    /// demand.
    pub fn exclusive(path: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path)?;
        file.lock_exclusive()?;
        Ok(PathLock { file })
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        self.file.unlock().expect("unlock");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::thread;

    #[test]
    fn test_path_lock() -> io::Result<()> {
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
}
