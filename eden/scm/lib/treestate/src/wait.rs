/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::path::Path;
use std::path::PathBuf;

use fs_err as fs;
use types::hgid::NULL_ID;
use types::HgId;

/// State to wait for dirstate changes.
pub struct Wait {
    // The dirstate path.
    path: PathBuf,
    // The dirstate file content (expected to be short).
    raw: Box<[u8]>,
}

impl Wait {
    /// Construct a `Wait` to detect parent changes.
    /// `dot_path` is the working copy `.sl` directory.
    pub fn from_dot_dir(dot_path: &Path) -> Self {
        let dirstate_path = dot_path.join("dirstate");
        Self::from_dirstate_path(dirstate_path)
    }

    fn from_dirstate_path(path: PathBuf) -> Self {
        let raw = read_raw(&path);
        Self { path, raw }
    }

    /// Block until a parent change (p1 or p2) happens on disk.
    pub fn wait_for_parent_change(&mut self) -> io::Result<()> {
        let mut file_wait = atomicfile::Wait::from_path(&self.path)?;
        let old_parents = extract_parents(&self.raw);
        loop {
            let new_raw = read_raw(&self.path);
            if extract_parents(&new_raw) != old_parents {
                self.raw = new_raw;
                break;
            }
            // Block
            file_wait.wait_for_change()?;
        }
        Ok(())
    }

    /// Test if anything in the dirstate might have changed. Does not block.
    /// Updates self so the same change won't be reported twice.
    pub fn is_dirstate_changed(&mut self) -> bool {
        let new_raw = read_raw(&self.path);
        let changed = self.raw != new_raw;
        if changed {
            self.raw = new_raw;
        }
        changed
    }

    /// Obtain the first parent.
    pub fn p1(&self) -> HgId {
        extract_parents(&self.raw)[0]
    }
}

fn read_raw(path: &Path) -> Box<[u8]> {
    match fs::read(path) {
        Ok(v) => v.into(),
        Err(_) => Default::default(),
    }
}

fn extract_parents(raw: &[u8]) -> [types::hgid::HgId; 2] {
    if let Some(slice) = raw.get(0..HgId::len() * 2) {
        [
            HgId::from_slice(&slice[..HgId::len()]).unwrap(),
            HgId::from_slice(&slice[HgId::len()..]).unwrap(),
        ]
    } else {
        [NULL_ID; 2]
    }
}
