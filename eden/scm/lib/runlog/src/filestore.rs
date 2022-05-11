/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::Error;
use anyhow::Result;
use fs2::FileExt;
use util::lock::PathLock;
use util::path::create_shared_dir;

use crate::Entry;

pub struct FileStore {
    // Directory to write files to (normally .hg/runlog/).
    dir: PathBuf,

    // Path to runlog watchfile (normally .hg/runlog_watchfile).
    watchfile_path: PathBuf,

    // A lock file indicating we are still running.
    #[allow(dead_code)]
    lock_file: PathLock,

    // Whether the current command is boring. This determines whether
    // we touch the watchfile on updates and how aggressively we clean
    // up files.
    boring: bool,
}

const LOCK_EXT: &str = "lock";
const JSON_EXT: &str = "json";
const WATCHFILE: &str = "runlog_watchfile";
const RUNLOG_DIR: &str = "runlog";

/// FileStore is a simple runlog storage that writes JSON entries to a
/// specified directory.
impl FileStore {
    // Create a new FileStore that writes files to directory dir. dir
    // is created automatically if it doesn't exist.
    pub(crate) fn new(shared_dot_hg_dir: &Path, entry_id: &str, boring: bool) -> Result<Self> {
        let dir = shared_dot_hg_dir.join(RUNLOG_DIR);

        create_shared_dir(&dir)?;

        let lock_file = PathLock::exclusive(dir.join(entry_id).with_extension(LOCK_EXT))?;

        Ok(FileStore {
            dir,
            watchfile_path: shared_dot_hg_dir.join(WATCHFILE),
            lock_file,
            boring,
        })
    }

    pub(crate) fn save(&self, e: &Entry) -> Result<()> {
        // NB: we don't fsync so incomplete or empty JSON files are possible.
        util::file::atomic_write(&self.dir.join(&e.id).with_extension(JSON_EXT), |f| {
            serde_json::to_writer_pretty(f, e)?;
            Ok(())
        })?;

        if !self.boring && !e.command.is_empty() {
            // Contents aren't important, but it makes it easier to test.
            fs::write(&self.watchfile_path, &e.command[0])?;
        }

        Ok(())
    }

    pub(crate) fn close(&self, e: &Entry) -> Result<()> {
        // Remove inconsequential, clean-exitting runlog entries immediately.
        if self.boring && e.exit_code == Some(0) {
            let path = self.dir.join(&e.id);
            remove_file_ignore_missing(path.with_extension(LOCK_EXT))?;
            remove_file_ignore_missing(path.with_extension(JSON_EXT))?;
        }
        Ok(())
    }

    pub(crate) fn cleanup<P: AsRef<Path>>(shared_dot_hg_dir: P, threshold: Duration) -> Result<()> {
        let dir = shared_dot_hg_dir.as_ref().join(RUNLOG_DIR);

        create_shared_dir(&dir)?;

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

            // Command process is still running - don't clean up.
            if is_locked(&path)? {
                continue;
            }

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

    // Iterates each entry, yielding the entry and whether the
    // associated command is still running.
    pub fn entry_iter<P: AsRef<Path>>(
        shared_dot_hg_path: P,
    ) -> Result<impl Iterator<Item = Result<(Entry, bool), Error>>> {
        let dir = shared_dot_hg_path.as_ref().join(RUNLOG_DIR);

        create_shared_dir(&dir)?;

        Ok(fs::read_dir(&dir)?.filter_map(|file| match file {
            Ok(file) => {
                // We only care about ".json" files.
                match file.path().extension().and_then(OsStr::to_str) {
                    Some(ext) if ext == JSON_EXT => {}
                    _ => return None,
                };

                match fs::File::open(file.path()) {
                    Ok(f) => Some(
                        serde_json::from_reader(&f)
                            .map_err(Error::new)
                            .and_then(|e| Ok((e, is_locked(file.path())?))),
                    ),
                    Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                    Err(err) => Some(Err(Error::new(err))),
                }
            }
            Err(err) => Some(Err(Error::new(err))),
        }))
    }
}

fn remove_file_ignore_missing<P: AsRef<Path>>(path: P) -> io::Result<()> {
    fs::remove_file(&path).or_else(|err| match err.kind() {
        io::ErrorKind::NotFound => Ok(()),
        _ => Err(err),
    })
}

// Return whether path's corresponding locked file is exclusively
// locked (by running command). Return false if lock file doesn't exist.
fn is_locked<P: AsRef<Path>>(path: P) -> Result<bool> {
    match fs::File::open(path.as_ref().with_extension(LOCK_EXT)) {
        Ok(f) => Ok(f.try_lock_shared().is_err()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(Error::new(err)),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::prelude::MetadataExt;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_save() -> Result<()> {
        let td = tempdir()?;

        let fs = FileStore::new(td.path(), "some_id", false)?;
        let rl_dir = td.path().join(RUNLOG_DIR);

        // Make sure FileStore creates directory automatically.
        assert!(rl_dir.exists());

        let mut entry = Entry::new(vec!["some_command".to_string()]);

        let assert_entry = |e: &Entry| {
            let f = fs::File::open(rl_dir.join(&e.id).with_extension(JSON_EXT)).unwrap();
            let got: Entry = serde_json::from_reader(&f).unwrap();
            assert_eq!(&got, e);

            #[cfg(unix)]
            assert_eq!(0o644, 0o777 & f.metadata().unwrap().mode());
        };

        // Can create new entry.
        fs.save(&entry)?;
        assert_entry(&entry);

        // Can update existing entry.
        entry.pid = 1234;
        fs.save(&entry)?;
        assert_entry(&entry);

        Ok(())
    }

    #[test]
    fn test_cleanup() -> Result<()> {
        let td = tempdir()?;
        let e = Entry::new(vec!["foo".to_string()]);
        let entry_path = td
            .path()
            .join(RUNLOG_DIR)
            .join(&e.id)
            .with_extension(JSON_EXT);

        {
            let fs = FileStore::new(td.path(), &e.id, false)?;
            fs.save(&e)?;

            // Still locked, don't clean up.
            FileStore::cleanup(&td, Duration::ZERO)?;
            assert!(entry_path.exists());
        }

        // No longer locked since file store is closed, but haven't met threshold.
        FileStore::cleanup(&td, Duration::from_secs(3600))?;
        assert!(entry_path.exists());

        // Met threshold - delete.
        FileStore::cleanup(&td, Duration::ZERO)?;
        assert!(!entry_path.exists());

        // Don't delete the watchfile.
        assert!(td.path().join(WATCHFILE).exists());

        Ok(())
    }

    #[test]
    fn test_iter() -> Result<()> {
        let td = tempdir()?;

        let a = Entry::new(vec!["a".to_string()]);
        let a_fs = FileStore::new(td.path(), &a.id, false)?;
        a_fs.save(&a)?;

        let b = Entry::new(vec!["b".to_string()]);
        {
            let b_fs = FileStore::new(td.path(), &b.id, false)?;
            b_fs.save(&b)?;
        }

        let mut got: Vec<(Entry, bool)> = FileStore::entry_iter(td.path())?
            .map(Result::unwrap)
            .collect();
        got.sort_by(|a, b| a.0.command[0].cmp(&b.0.command[0]));

        assert_eq!(vec![(a, true), (b, false)], got);

        Ok(())
    }

    #[test]
    fn test_watchfile() -> Result<()> {
        let td = tempdir()?;

        // Should write out watchfile.
        {
            let e = Entry::new(vec!["exciting".to_string()]);
            let fs = FileStore::new(td.path(), &e.id, false)?;
            fs.save(&e)?;
        }

        let watchfile_path = td.path().join(WATCHFILE);
        assert_eq!(fs::read_to_string(&watchfile_path)?, "exciting");

        // Boring command, don't touch watchfile.
        {
            let e = Entry::new(vec!["boring".to_string()]);
            let fs = FileStore::new(td.path(), &e.id, true)?;
            fs.save(&e)?;
        }

        assert_eq!(fs::read_to_string(&watchfile_path)?, "exciting");

        // Should touch watchfile.
        {
            let e = Entry::new(vec!["amazing".to_string()]);
            let fs = FileStore::new(td.path(), &e.id, false)?;
            fs.save(&e)?;
        }

        assert_eq!(fs::read_to_string(&watchfile_path)?, "amazing");

        Ok(())
    }
}
