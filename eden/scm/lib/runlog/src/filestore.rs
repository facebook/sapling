/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};

use std::{fs, io, path::PathBuf};

use crate::Entry;

#[derive(Clone)]
pub struct FileStore(PathBuf);

/// FileStore is a simple runlog storage that writes JSON entries to a
/// specified directory.
impl FileStore {
    // Create a new FileStore that writes files to directory p. p is
    // created automatically if it doesn't exist.
    pub(crate) fn new(p: PathBuf) -> Result<Self> {
        match fs::create_dir(&p) {
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(anyhow!(err)),
            Ok(_) => {}
        }
        return Ok(FileStore(p));
    }

    pub(crate) fn save(&self, e: &Entry) -> Result<()> {
        // Write to temp file and rename to avoid incomplete writes.
        let mut tmp = tempfile::NamedTempFile::new_in(&self.0)?;
        serde_json::to_writer_pretty(&tmp, e)?;
        tmp.as_file_mut().sync_data()?;
        tmp.persist(self.0.join(&e.id))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_save() {
        let td = tempdir().unwrap();

        let fs_dir = td.path().join("banana");
        let fs = FileStore::new(fs_dir.clone()).unwrap();
        // Make sure FileStore creates directory automatically.
        assert!(fs_dir.exists());

        let mut entry = Entry::new(vec!["some_command".to_string()]);

        let assert_entry = |e: &Entry| {
            let f = fs::File::open(fs_dir.join(&e.id)).unwrap();
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
}
