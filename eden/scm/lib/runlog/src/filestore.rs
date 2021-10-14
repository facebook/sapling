/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use fs2::FileExt;

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, SystemTime},
};

use crate::Entry;
use util::{lock::PathLock, path::create_shared_dir};

pub struct FileStore {
    // Directory to write files to.
    dir: PathBuf,

    // A lock file indicating we are still running.
    #[allow(dead_code)]
    lock_file: PathLock,
}

const LOCK_EXT: &str = "lock";
const JSON_EXT: &str = "json";

/// FileStore is a simple runlog storage that writes JSON entries to a
/// specified directory.
impl FileStore {
    // Create a new FileStore that writes files to directory dir. dir
    // is created automatically if it doesn't exist.
    pub(crate) fn new(dir: PathBuf, entry_id: &str) -> Result<Self> {
        create_shared_dir(&dir)?;

        let lock_file = PathLock::exclusive(dir.join(entry_id).with_extension(LOCK_EXT))?;

        Ok(FileStore { dir, lock_file })
    }

    pub(crate) fn save(&mut self, e: &Entry) -> Result<()> {
        // Retry a few times since renaming file fails on windows if
        // destination path exists and is open.
        let mut retries = 3;
        loop {
            let res = self.save_attempt(e);
            if retries == 0 || res.is_ok() {
                break res;
            }
            retries -= 1;
            sleep(Duration::from_millis(5));
        }
    }

    fn save_attempt(&mut self, e: &Entry) -> Result<()> {
        // Write to temp file and rename to avoid incomplete writes.
        let tmp = tempfile::NamedTempFile::new_in(&self.dir)?;

        serde_json::to_writer_pretty(&tmp, e)?;

        // NB: we don't fsync so incomplete or empty JSON files are possible.

        tmp.persist(self.dir.join(&e.id).with_extension(JSON_EXT))?;

        Ok(())
    }

    pub(crate) fn cleanup<P: AsRef<Path>>(dir: P, threshold: Duration) -> Result<()> {
        if !dir.as_ref().exists() {
            return Ok(());
        }

        for dir_entry in fs::read_dir(dir)? {
            let path = dir_entry?.path();

            let ext = match path.extension().and_then(OsStr::to_str) {
                Some(ext) => ext,
                _ => continue,
            };

            // Skip ".lock" files. This leaves ".json" and any stray tmp files.
            if ext == LOCK_EXT {
                continue;
            }

            match fs::File::open(path.with_extension(LOCK_EXT)) {
                Ok(f) => {
                    // Command process is still running - don't clean up.
                    if f.try_lock_shared().is_err() {
                        continue;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(Error::new(err)),
            };


            // Avoid trying to read the contents so we can clean up
            // incomplete files.
            let mtime = fs::metadata(&path)?.modified()?;
            if SystemTime::now().duration_since(mtime)? >= threshold {
                // Cleanup up ".json" or tmp file.
                remove_file_ignore_missing(&path)?;
                // Clean up lock file (we know command process isn't running anymore).
                remove_file_ignore_missing(path.with_extension(LOCK_EXT))?;
            }
        }

        Ok(())
    }
}

fn remove_file_ignore_missing<P: AsRef<Path>>(path: P) -> io::Result<()> {
    fs::remove_file(&path).or_else(|err| match err.kind() {
        io::ErrorKind::NotFound => Ok(()),
        _ => Err(err),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_save() {
        let td = tempdir().unwrap();

        let fs_dir = td.path().join("banana");
        let mut fs = FileStore::new(fs_dir.clone(), "some_id").unwrap();
        // Make sure FileStore creates directory automatically.
        assert!(fs_dir.exists());

        let mut entry = Entry::new(vec!["some_command".to_string()]);

        let assert_entry = |e: &Entry| {
            let f = fs::File::open(fs_dir.join(&e.id).with_extension(JSON_EXT)).unwrap();
            let got: Entry = serde_json::from_reader(&f).unwrap();
            assert_eq!(&got, e);
        };

        // Can create new entry.
        fs.save(&entry).unwrap();
        assert_entry(&entry);

        // Can update existing entry.
        entry.pid = 1234;
        fs.save(&entry).unwrap();
        assert_entry(&entry);
    }

    #[test]
    fn test_cleanup() {
        let td = tempdir().unwrap();
        let e = Entry::new(vec!["foo".to_string()]);
        let entry_path = td.path().join(&e.id).with_extension(JSON_EXT);

        {
            let mut fs = FileStore::new(td.path().into(), &e.id).unwrap();
            fs.save(&e).unwrap();

            // Still locked, don't clean up.
            FileStore::cleanup(&td, Duration::ZERO).unwrap();
            assert!(entry_path.exists());
        }

        // No longer locked since file store is closed, but haven't met threshold.
        FileStore::cleanup(&td, Duration::from_secs(3600)).unwrap();
        assert!(entry_path.exists());

        // Met threshold - delete.
        FileStore::cleanup(&td, Duration::ZERO).unwrap();
        assert!(!entry_path.exists());
    }
}
