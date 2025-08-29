/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(type_alias_impl_trait)]

use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use fs_err as fs;
use serde::Serialize;
use util::file::get_umask;
use util::lock::ContentLock;
use util::lock::ContentLockError;
use util::lock::PathLock;
use util::path::create_dir_all_with_mode;
use util::path::dir_mode;
use util::path::remove_file;

const ASSERTED_STATE_DIR: &str = ".edenfs-notifications-state";

fn ensure_directory(path: &Path) -> anyhow::Result<()> {
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

#[derive(thiserror::Error, Debug)]
pub enum StateError {
    #[error("State is already asserted {0}")]
    StateAlreadyAsserted(String),
    #[error("IO error {0}")]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    ContentLockError(#[from] ContentLockError),
    #[error("{0}")]
    OtherError(#[from] anyhow::Error),
}

impl StreamingChangesClient {
    pub fn new(mount_point: &Path) -> Result<Self, StateError> {
        let states_root = mount_point.join(ASSERTED_STATE_DIR);
        ensure_directory(&states_root)?;

        Ok(StreamingChangesClient { states_root })
    }

    #[allow(dead_code)]
    pub fn get_state_path(&self, state: &str) -> Result<PathBuf, StateError> {
        let state_path = self.states_root.join(state);
        ensure_directory(&state_path)?;
        Ok(state_path)
    }

    pub fn enter_state(&self, state: &str) -> Result<ContentLockGuard, StateError> {
        // Asserts the named state, in the current mount.
        // Returns () if the state was successfully asserted, or an StateAlreadyAsserted StateError if the state was already asserted.
        // Returns other errors if an error occurred while asserting the state.
        // To exit the state, drop the ContentLockGuard returned by this function either explicitly
        // or implicitly by letting it go out of scope.
        // TODO: Add logging
        let state_path: PathBuf = self.get_state_path(state)?;
        match try_lock_state(&state_path, state) {
            Ok(lock) => Ok(lock),
            Err(ContentLockError::Contended(_)) => {
                Err(StateError::StateAlreadyAsserted(state.to_string()))
            }
            Err(ContentLockError::Io(err)) => Err(err.into()),
        }
    }

    pub fn get_asserted_states(&self) -> Result<HashSet<String>, StateError> {
        // Gets a set of all asserted states.
        // For use in debug CLI. Not intended for end user consumption,
        // use is_state_asserted() with your list of states instead.
        let mut asserted_states = HashSet::new();
        for dir_entry in fs::read_dir(&self.states_root)? {
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

    pub fn is_state_asserted(&self, state: &str) -> Result<bool, StateError> {
        let state_path = self.get_state_path(state)?;
        match is_state_locked(&state_path, state) {
            Ok(true) => Ok(true),
            Ok(false) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    #[allow(dead_code)]
    fn which_states_asserted(&self, states: &[String]) -> Result<HashSet<String>, StateError> {
        let mut output = HashSet::new();
        for state in states {
            if self.is_state_asserted(state)? {
                output.insert(state.clone());
            }
        }
        Ok(output)
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
fn is_state_locked(dir: &Path, name: &str) -> Result<bool, ContentLockError> {
    // Check the lock state, without creating the lock file
    // If the lock doesn't exist, return false
    let content_lock = ContentLock::new_with_name(dir, name);
    match content_lock.check_lock() {
        Ok(()) => Ok(false),
        Err(ContentLockError::Contended(_)) => Ok(true),
        Err(ContentLockError::Io(err)) => Err(err.into()),
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub enum StateChange {
    Entered,
    Left,
}

impl fmt::Display for StateChange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self == &StateChange::Entered {
            write!(f, "Entered")
        } else {
            write!(f, "Left")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_enter_state() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state = "test_state1";
        let _result = client.enter_state(state)?;
        let check_state = client.is_state_asserted(state)?;
        assert!(check_state);
        Ok(())
    }

    #[test]
    fn test_state_leave() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount1");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state = "test_state2";
        let guard = client.enter_state(state)?;
        let check_state = client.is_state_asserted(state)?;
        assert!(check_state);
        drop(guard);
        let exited_state = client.is_state_asserted(state)?;
        assert!(!exited_state);
        Ok(())
    }

    #[test]
    fn test_state_leave_implicit() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state = "test_state2";
        {
            let _guard = client.enter_state(state)?;
            let check_state = client.is_state_asserted(state)?;
            assert!(check_state);
        }
        let exited_state = client.is_state_asserted(state)?;
        assert!(!exited_state);
        Ok(())
    }

    #[test]
    fn test_get_asserted_states_empty() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount2");
        let client = StreamingChangesClient::new(&mount_point)?;
        let asserted_states = client.get_asserted_states()?;
        assert!(asserted_states.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_asserted_states() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount3");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state1 = "test_state1";
        let state2 = "test_state2";

        let guard_result = client.enter_state(state1)?;
        let guard_result2 = client.enter_state(state2)?;
        let asserted_states = client.get_asserted_states()?;
        assert_eq!(
            asserted_states,
            HashSet::from([state1.to_string(), state2.to_string()])
        );

        drop(guard_result);
        let asserted_states = client.get_asserted_states()?;
        assert_eq!(asserted_states, HashSet::from([state2.to_string()]));
        drop(guard_result2);
        let asserted_states = client.get_asserted_states()?;
        assert!(asserted_states.is_empty());
        Ok(())
    }

    #[test]
    fn test_try_guarded_lock() -> anyhow::Result<()> {
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

    #[test]
    fn test_multiple_mount() -> anyhow::Result<()> {
        let mount_point1 = std::env::temp_dir().join("test_mount4");
        let mount_point2 = std::env::temp_dir().join("test_mount4a");
        let client1 = StreamingChangesClient::new(&mount_point1)?;
        let client2 = StreamingChangesClient::new(&mount_point2)?;
        let state1 = "test_state1";
        let state2 = "test_state2";
        let guard_result = client1.enter_state(state1)?;
        let _guard_result2 = client2.enter_state(state2)?;
        let asserted_states = client1.get_asserted_states()?;
        assert_eq!(asserted_states, HashSet::from([state1.to_string()]));
        let asserted_states = client2.get_asserted_states()?;
        assert_eq!(asserted_states, HashSet::from([state2.to_string()]));

        drop(guard_result);
        let asserted_states = client1.get_asserted_states()?;
        assert!(asserted_states.is_empty());
        let asserted_states = client2.get_asserted_states()?;
        assert_eq!(asserted_states, HashSet::from([state2.to_string()]));
        Ok(())
    }

    #[test]
    fn test_repeat_enter() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount6");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state = "test_state";
        let result = client.enter_state(state);
        let result2 = client.enter_state(state);
        assert!(result.is_ok());
        match result2 {
            Ok(_) => return Err(anyhow::anyhow!("State should not be asserted twice")),
            Err(StateError::StateAlreadyAsserted(_)) => {}
            _ => {
                return Err(anyhow::anyhow!(
                    "State should return StateAlreadyAsserted error"
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn test_try_lock_state() -> anyhow::Result<()> {
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

    #[test]
    fn test_states_asserted() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount7");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state = "test_state";
        let state2 = "test_state2";
        let guard_result = client.enter_state(state)?;
        let states_asserted = client.which_states_asserted(&[state.to_string()])?;
        assert!(!states_asserted.is_empty());
        let states_asserted = client.which_states_asserted(&[state2.to_string()])?;
        assert!(states_asserted.is_empty());
        drop(guard_result);
        let states_asserted = client.which_states_asserted(&[state.to_string()])?;
        assert!(states_asserted.is_empty());
        Ok(())
    }
}
