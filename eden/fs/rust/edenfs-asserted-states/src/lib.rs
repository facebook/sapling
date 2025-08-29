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
use std::time::Duration;

use edenfs_client::changes_since::ChangeNotification;
use edenfs_client::changes_since::ChangesSinceV2Result;
use edenfs_client::changes_since::SmallChangeNotification;
use edenfs_client::types::JournalPosition;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::path_ref_from_bytes;
use fs_err as fs;
use futures::StreamExt;
use futures::stream;
use futures::stream::BoxStream;
use serde::Serialize;
use util::file::get_umask;
use util::lock::ContentLock;
use util::lock::ContentLockError;
use util::lock::PathLock;
use util::path::create_dir_all_with_mode;
use util::path::dir_mode;
use util::path::remove_file;

const ASSERTED_STATE_DIR: &str = ".edenfs-notifications-state";

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

#[derive(thiserror::Error, Debug)]
pub enum StateError {
    #[error(transparent)]
    EdenFsError(#[from] EdenFsError),
    #[error("State is already asserted {0}")]
    StateAlreadyAsserted(String),
    #[error("{0}")]
    OtherError(#[from] anyhow::Error),
}

impl StreamingChangesClient {
    pub fn new(mount_point: &Path) -> Result<Self> {
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

    pub fn enter_state(&self, state: &str) -> Result<ContentLockGuard, StateError> {
        // Asserts the named state, in the current mount.
        // Returns () if the state was successfully asserted, or an StateAlreadyAsserted StateError if the state was already asserted.
        // Returns other errors if an error occurred while asserting the state.
        // To exit the state, drop the ContentLockGuard returned by this function either explicitly
        // or implicitly by letting it go out of scope.
        // TODO: Add logging
        let state_path: PathBuf = self
            .get_state_path(state)
            .map_err(StateError::EdenFsError)?;
        match try_lock_state(&state_path, state) {
            Ok(lock) => Ok(lock),
            Err(ContentLockError::Contended(_)) => {
                Err(StateError::StateAlreadyAsserted(state.to_string()))
            }
            Err(ContentLockError::Io(err)) => Err(StateError::EdenFsError(EdenFsError::from(err))),
        }
    }

    pub fn get_asserted_states(&self) -> Result<HashSet<String>> {
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

    pub fn is_state_asserted(&self, state: &str) -> Result<bool> {
        let state_path = self.get_state_path(state)?;
        match is_state_locked(&state_path, state) {
            Ok(true) => Ok(true),
            Ok(false) => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub async fn stream_changes_since_with_states<'a>(
        &'a self,
        inner_stream: BoxStream<'a, Result<ChangesSinceV2Result>>,
        states: &'a [String],
    ) -> Result<BoxStream<'a, Result<(ChangesSinceV2Result, ChangeEvents)>>> {
        let state_data = StreamChangesSinceWithStatesData {
            inner_stream: inner_stream.boxed(),
            last_event: None,
            state: IsStateCurrentlyAsserted::NotAsserted,
            asserted_states: HashSet::new(),
            position: JournalPosition::default(),
        };

        let stream = stream::unfold(state_data, move |mut state_data| async move {
            match state_data.state {
                IsStateCurrentlyAsserted::NotAsserted => {
                    let next_result = state_data.inner_stream.as_mut().next().await?;
                    match next_result {
                        Ok(inner_result) => {
                            let (change_events, state_entered_index) = self
                                .get_state_changes_and_first_entered_index(&inner_result, states);
                            if let Some(state_entered_index) = state_entered_index {
                                for change_event in &change_events.events {
                                    match change_event.event_type {
                                        StateChange::Entered => {
                                            state_data
                                                .asserted_states
                                                .insert(change_event.state.clone());
                                        }
                                        StateChange::Left => {
                                            // May happen if the state was entered and exited in the same
                                            // result, or if a preexisting state was used
                                            state_data
                                                .asserted_states
                                                .remove(&change_event.state.clone());
                                        }
                                    }
                                }
                                if state_data.asserted_states.is_empty() {
                                    let output = Ok((inner_result, change_events));
                                    return Some((output, state_data));
                                }
                                let (left_changes, right_changes) =
                                    inner_result.changes.split_at(state_entered_index);
                                let to_position = inner_result.to_position.clone();
                                let pre_enter_result = ChangesSinceV2Result {
                                    to_position: to_position.clone(),
                                    changes: left_changes.to_vec(),
                                };
                                let post_enter_result = ChangesSinceV2Result {
                                    to_position: to_position.clone(),
                                    changes: right_changes.to_vec(),
                                };
                                state_data.position = to_position.clone();
                                state_data.last_event = Some(post_enter_result);
                                state_data.state = IsStateCurrentlyAsserted::StateAsserted;
                                let output = Ok((pre_enter_result, change_events));
                                Some((output, state_data))
                            } else {
                                // No state entered, return the result directly
                                let output = Ok((inner_result, change_events));
                                Some((output, state_data))
                            }
                        }
                        Err(e) => {
                            // Pass through the error
                            Some((Err(e), state_data))
                        }
                    }
                }
                IsStateCurrentlyAsserted::StateAsserted => {
                    let timer = tokio::time::interval(Duration::from_secs(1));
                    tokio::pin!(timer);
                    loop {
                        let mut change_events = ChangeEvents::new();
                        tokio::select! {
                            _ = timer.tick() => {
                                // Check states, to see if any have been deasserted without a notification due to crash.
                                // May occasionally send a double exit if the timer hits immediately before an entry containing
                                // an exit from the stream, but clients should ignore the second one.
                                // Does not check for newly entered states, those should be reliably handled via
                                // the journal.
                                if let Ok(asserted_states) = self.which_states_asserted(states) {
                                    let left_states: Vec<_> = state_data
                                        .asserted_states
                                        .difference(&asserted_states)
                                        .cloned()
                                        .collect();
                                    if !left_states.is_empty() {
                                        for difference in left_states {
                                            tracing::debug!("Found deasserted state during timer check: {:?}", difference);
                                            change_events.events.push(ChangeEvent {
                                                event_type: StateChange::Left,
                                                state: difference.to_string(),
                                                position: state_data.position.clone(),
                                            });
                                            state_data.asserted_states.remove(&difference);
                                        }
                                        let mut results = ChangesSinceV2Result {
                                            to_position: state_data.position.clone(),
                                            changes: Vec::new(),
                                        };
                                        if asserted_states.is_empty() {
                                            state_data.state = IsStateCurrentlyAsserted::NotAsserted;
                                            results = state_data.last_event.take().unwrap_or(results);
                                        }
                                        let output = Ok((results, change_events));
                                        return Some((output, state_data));
                                    }
                                }
                            },
                            next_result_opt = state_data.inner_stream.next() => {
                                match next_result_opt {
                                    None => {
                                        // Stream terminated
                                        return None;
                                    }
                                    Some(next_result) => {
                                        match next_result {
                                            Ok(mut inner_result) => {
                                                (change_events, _) = self.get_state_changes_and_first_entered_index(&inner_result, states);
                                                for change_event in &change_events.events {
                                                    match change_event.event_type {
                                                        StateChange::Entered => {
                                                            state_data
                                                                .asserted_states
                                                                .insert(change_event.state.clone());
                                                        }
                                                        StateChange::Left => {
                                                            state_data
                                                                .asserted_states
                                                                .remove(&change_event.state);
                                                        }
                                                    }
                                                }

                                                // At this point, last_event should already contain a value
                                                let left_changes =
                                                    state_data
                                                        .last_event
                                                        .take()
                                                        .unwrap_or(ChangesSinceV2Result {
                                                            to_position: JournalPosition::default(),
                                                            changes: Vec::new(),
                                                        });
                                                let mut merged_changes = ChangesSinceV2Result {
                                                    to_position: inner_result.to_position,
                                                    changes: left_changes.changes,
                                                };
                                                merged_changes.changes.append(&mut inner_result.changes);
                                                state_data.position = merged_changes.to_position.clone();

                                                if state_data.asserted_states.is_empty() {
                                                    state_data.state = IsStateCurrentlyAsserted::NotAsserted;
                                                    let results = Ok((merged_changes, change_events));
                                                    return Some((results, state_data));
                                                } else {
                                                    state_data.last_event = Some(merged_changes);
                                                    let results = Ok((
                                                        ChangesSinceV2Result {
                                                            to_position: state_data.position.clone(),
                                                            changes: Vec::new(),
                                                        }, change_events));
                                                    return Some((results, state_data));
                                                }
                                            }
                                            Err(e) => {
                                                // Pass through the error. The stream will be terminated
                                                // inside the subscription after surfacing an error.
                                                tracing::error!("error while checking states {:?}", e);
                                                return Some((Err(e), state_data));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Ok(stream.boxed())
    }

    fn which_states_asserted(&self, states: &[String]) -> Result<HashSet<String>> {
        let mut output = HashSet::new();
        for state in states {
            if self.is_state_asserted(state)? {
                output.insert(state.clone());
            }
        }
        Ok(output)
    }

    fn get_state_changes_and_first_entered_index<'a>(
        &'a self,
        changes: &'a ChangesSinceV2Result,
        states: &[String],
    ) -> (ChangeEvents, Option<usize>) {
        let mut change_events = ChangeEvents::new();
        let mut first_entered_index = None;
        for (i, change) in changes.changes.iter().enumerate() {
            if let ChangeNotification::SmallChange(small_change) = change {
                // Ignore the Err that happens due to invalid UTF-8, values from eden should
                // only be UTF-8
                if let Ok(path) = path_ref_from_bytes(small_change.first_path()) {
                    if let Some(state_name) = self.get_notify_file_state(path, states) {
                        match small_change {
                            SmallChangeNotification::Added(_) => {
                                tracing::debug!(
                                    "Entered state {:?} at {}",
                                    path,
                                    changes.to_position.clone()
                                );
                                change_events.events.push(ChangeEvent {
                                    event_type: StateChange::Entered,
                                    state: state_name.to_string(),
                                    position: changes.to_position.clone(),
                                });
                                if first_entered_index.is_none() {
                                    first_entered_index = Some(i);
                                }
                            }
                            SmallChangeNotification::Removed(_)
                            | SmallChangeNotification::Renamed(_) => {
                                tracing::debug!(
                                    "Left state {:?} at {}",
                                    path,
                                    changes.to_position.clone()
                                );
                                change_events.events.push(ChangeEvent {
                                    event_type: StateChange::Left,
                                    state: state_name.to_string(),
                                    position: changes.to_position.clone(),
                                });
                            }
                            SmallChangeNotification::Modified(_) => {
                                // Modified state file happens on linux platforms immediately after creation.
                                // Ignore it, since it doesn't change the state
                            }
                            SmallChangeNotification::Replaced(_) => {
                                // We currently do not expect to see Replaced happening on the
                                // state notifier file
                                tracing::debug!(
                                    "Unexpected state change Replaced in {:?} at {}",
                                    path,
                                    changes.to_position.clone()
                                );
                            }
                        }
                    }
                }
            }
        }
        (change_events, first_entered_index)
    }

    fn is_path_notify_file(&self, path: &Path) -> bool {
        // Check if the item at path is a valid lockfile
        // A valid lockfile is a non-directory that exists and ends with .notify inside states_root
        if !path.starts_with(ASSERTED_STATE_DIR) {
            return false;
        }
        match path.extension() {
            Some(ext) => ext == "notify",
            None => false,
        }
    }

    fn get_notify_file_state<'a>(&self, path: &'a Path, states: &[String]) -> Option<&'a str> {
        // Check if the lockfile at path is one of the states we care about
        if !self.is_path_notify_file(path) {
            return None;
        }
        if let Some(state) = self.get_state_name(path) {
            if states.iter().any(|s| s == state) {
                return Some(state);
            }
        }
        None
    }

    fn get_state_name<'a>(&self, path: &'a Path) -> Option<&'a str> {
        // Get the name of the state from the path
        // The state name is the parent directory of the notify file
        match path.parent() {
            Some(parent) => match parent.file_name() {
                Some(state_name) => state_name.to_str(),
                None => None,
            },
            None => None,
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

enum IsStateCurrentlyAsserted {
    NotAsserted,
    StateAsserted,
}

struct StreamChangesSinceWithStatesData<'a> {
    inner_stream: BoxStream<'a, Result<ChangesSinceV2Result>>,
    last_event: Option<ChangesSinceV2Result>,
    state: IsStateCurrentlyAsserted,
    asserted_states: HashSet<String>,
    position: JournalPosition,
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

#[derive(Debug, Serialize)]
pub struct ChangeEvent {
    event_type: StateChange,
    state: String,
    position: JournalPosition,
}

impl fmt::Display for ChangeEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} at {}", self.event_type, self.state, self.position)
    }
}

#[derive(Debug, Serialize)]
pub struct ChangeEvents {
    events: Vec<ChangeEvent>,
}

impl fmt::Display for ChangeEvents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for event in self.events.iter() {
            writeln!(f, "{event}")?;
        }
        Ok(())
    }
}

impl ChangeEvents {
    pub fn new() -> Self {
        ChangeEvents { events: Vec::new() }
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

    #[test]
    fn test_is_path_notify_file() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount9");
        let client = StreamingChangesClient::new(&mount_point)?;
        let state = "test_state";
        let state_dir = PathBuf::from(ASSERTED_STATE_DIR);
        let _guard_result = client.enter_state(state)?;
        assert!(!client.is_path_notify_file(&state_dir));
        assert!(!client.is_path_notify_file(&state_dir.join(state)));
        assert!(!client.is_path_notify_file(&state_dir.join(state).join(state)));
        assert!(
            client.is_path_notify_file(&state_dir.join(state).join(state).with_extension("notify"))
        );
        Ok(())
    }
}
