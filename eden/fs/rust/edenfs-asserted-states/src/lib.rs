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

#[allow(dead_code)]
pub struct StreamingChangesClient {
    mount_point: PathBuf,
}

impl StreamingChangesClient {
    pub fn new(mount_point: PathBuf) -> Self {
        StreamingChangesClient { mount_point }
    }

    pub fn state_enter(&self, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn state_leave(&self, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn get_asserted_states(&self) -> Result<HashSet<String>> {
        Ok(HashSet::new())
    }

    pub fn is_state_asserted(&self, _state: &str) -> Result<bool> {
        Ok(false)
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

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;

    use crate::*;

    #[fbinit::test]
    fn test_get_asserted_states_empty(_fb: FacebookInit) -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount");
        let client = StreamingChangesClient::new(mount_point);
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
}
