/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use edenfs_error::Result;
use fs_err as fs;
use util::file::get_umask;
use util::lock::ContentLock;
use util::lock::ContentLockError;
use util::lock::PathLock;
use util::path::create_dir_all_with_mode;
use util::path::dir_mode;
use util::path::remove_file;

const ASSERTED_STATE_DIR: &str = ".edenfs-notifications-state";

#[allow(dead_code)]
fn ensure_directory(path: &Path) -> Result<()> {
    // Create the directory, if it doesn't exist.
    match path.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            create_dir_all_with_mode(path, dir_mode(get_umask()))?;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

pub struct StreamingChangesClient {
    states_root: PathBuf,
}

#[allow(dead_code)]
impl StreamingChangesClient {
    pub fn new(mount_point: PathBuf) -> Result<Self> {
        let states_root = mount_point.join(ASSERTED_STATE_DIR);
        ensure_directory(&states_root)?;

        Ok(StreamingChangesClient { states_root })
    }

    #[allow(dead_code)]
    pub fn get_state_path(&self, state: &str) -> Result<PathBuf> {
        let state_path = self.states_root.join(state);
        ensure_directory(&state_path)?;
        Ok(state_path)
    }

    pub fn state_enter(&self, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn state_leave(&self, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn get_asserted_states(&self) -> Result<HashSet<String>> {
        // Gets a set of all asserted states.
        // For use in debug CLI. Not intended for end user consumption,
        // use is_state_asserted() with your list of states instead.
        let mut asserted_states = HashSet::new();
        for dir_entry in std::fs::read_dir(&self.states_root)? {
            let entry = dir_entry?;
            if entry.path().is_dir() {
                let state = entry.file_name().to_string_lossy().to_string();
                if self.is_state_asserted(&state)? {
                    asserted_states.insert(state);
                }
            }
        }
        Ok(asserted_states)
    }

    pub fn is_state_asserted(&self, state: &str) -> Result<bool> {
        let state_path = self.get_state_path(state)?;
        match is_state_locked(&state_path, state) {
            Ok(true) => Ok(true),
            Ok(false) => Ok(false),
            Err(err) => Err(err),
        }
    }
}

// As PathLock, but creates an additional file with the .notify extension
// to log exit to the journal
#[derive(Debug)]
pub struct ContentLockGuard(PathLock);

impl Drop for ContentLockGuard {
    fn drop(&mut self) {
        // Done purely to signal the edenfs journal that the lock is no longer held.
        let file_path = self.0.as_file().path().with_extension("notify");
        match remove_file(&file_path) {
            Ok(_) => {}
            Err(e) => tracing::error!("Notify file {:?} missing: {:?}", file_path, e),
        };
        // Release the lock when the internal PathLock is dropped on exit
    }
}

pub fn try_guarded_lock(
    content_lock: &ContentLock,
    contents: &[u8],
) -> Result<ContentLockGuard, ContentLockError> {
    let inner_lock = content_lock.try_lock(contents)?;
    // Done purely to signal the edenfs journal that the lock has been acquired.
    let notify_file_path = inner_lock.as_file().path().with_extension("notify");
    if notify_file_path.exists() {
        remove_file(&notify_file_path)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(inner_lock.as_file().path().with_extension("notify"))?;
    Ok(ContentLockGuard(inner_lock))
}

#[allow(dead_code)]
fn try_lock_state(dir: &Path, name: &str) -> Result<ContentLockGuard, ContentLockError> {
    let content_lock = ContentLock::new_with_name(dir, name);
    let state_lock = try_guarded_lock(&content_lock, &[])?;

    Ok(state_lock)
}

#[allow(dead_code)]
fn is_state_locked(dir: &Path, name: &str) -> Result<bool> {
    // Check the lock state, without creating the lock file
    // If the lock doesn't exist, return false
    let content_lock = ContentLock::new_with_name(dir, name);
    match content_lock.check_lock() {
        Ok(()) => Ok(false),
        Err(ContentLockError::Contended(_)) => Ok(true),
        Err(ContentLockError::Io(err)) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;

    use crate::*;

    #[fbinit::test]
    fn test_get_asserted_states_empty(_fb: FacebookInit) -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount");
        let client = StreamingChangesClient::new(mount_point)?;
        let asserted_states = client.get_asserted_states()?;
        assert!(asserted_states.is_empty());
        Ok(())
    }

    #[fbinit::test]
    fn test_try_guarded_lock(_fb: FacebookInit) -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_try_lock_mount");
        let state = "test_state";
        let state_path = mount_point.join(state);
        ensure_directory(&state_path)?;
        let content_lock = ContentLock::new_with_name(&state_path, state);
        let guarded_lock = try_guarded_lock(&content_lock, b"")?;
        assert!(&state_path.join(state).exists());
        assert!(&state_path.join(state).with_extension("lock").exists());
        assert!(&state_path.join(state).with_extension("notify").exists());

        drop(guarded_lock);

        assert!(&state_path.join(state).exists());
        assert!(&state_path.join(state).with_extension("lock").exists());
        assert!(!&state_path.join(state).with_extension("notify").exists());
        Ok(())
    }

    #[fbinit::test]
    fn test_try_lock_state(_fb: FacebookInit) -> anyhow::Result<()> {
        let mount = "test_mount8";
        let state = "test.state";

        let mount_point = std::env::temp_dir().join(mount);
        let state_path = mount_point.join(state);

        ensure_directory(&state_path)?;
        let lock = try_lock_state(&state_path, state)?;
        assert!(is_state_locked(&state_path, state)?);
        drop(lock);
        assert!(!is_state_locked(&state_path, state)?);

        Ok(())
    }
}
