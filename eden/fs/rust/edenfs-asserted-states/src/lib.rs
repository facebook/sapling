/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use edenfs_asserted_states_client::AssertedStatesClient;
use edenfs_asserted_states_client::ContentLockGuard;
use edenfs_asserted_states_client::StateChange;
use edenfs_asserted_states_client::StateError;
use edenfs_client::changes_since::ChangeNotification;
use edenfs_client::changes_since::ChangesSinceV2Result;
use edenfs_client::changes_since::StateChangeNotification;
use edenfs_client::client::EdenFsClient;
use edenfs_client::types::JournalPosition;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_telemetry::EdenSample;
use edenfs_telemetry::QueueingScubaLogger;
use edenfs_telemetry::SampleLogger;
use edenfs_telemetry::create_logger;
use futures::StreamExt;
use futures::stream;
use futures::stream::BoxStream;
use itertools::Itertools;
use lazy_static::lazy_static;
use serde::Serialize;
use util::lock::unsanitize_lock_name;

lazy_static! {
    pub(crate) static ref SCUBA_CLIENT: QueueingScubaLogger =
        QueueingScubaLogger::new(create_logger("edenfs_client".to_string()), 1000);
}

#[derive(Clone, Debug)]
pub struct StreamingChangesClient {
    pub session_id: String,
    states_client: AssertedStatesClient,
}

pub fn get_streaming_changes_client(
    mount_point: &Path,
    eden_client: &EdenFsClient,
) -> Result<StreamingChangesClient> {
    StreamingChangesClient::new(mount_point, eden_client.get_session_id())
}

impl StreamingChangesClient {
    pub fn new(mount_point: &Path, session_id: String) -> Result<Self> {
        Ok(StreamingChangesClient {
            session_id,
            states_client: AssertedStatesClient::new(mount_point)
                .map_err(|e| EdenFsError::from(anyhow::Error::from(e)))?,
        })
    }

    pub fn instrument(&self, method: &str) {
        let mut sample = EdenSample::new();
        sample.add_string("method", method);
        sample.add_string("session_id", &self.session_id);
        sample.add_string("type", "notifications");
        sample.add_string("user", whoami::username());
        sample.add_string("host", whoami::fallible::hostname().unwrap_or_default());
        let _ = SCUBA_CLIENT.log(sample);
    }

    #[allow(dead_code)]
    pub fn get_state_path(&self, state: &str) -> Result<PathBuf> {
        self.states_client
            .get_state_path(state)
            .map_err(|e| EdenFsError::from(anyhow::Error::from(e)))
    }

    pub fn enter_state(&self, state: &str) -> Result<ContentLockGuard, StateError> {
        self.states_client.enter_state(state)
    }

    pub fn get_asserted_states(&self) -> Result<HashSet<String>> {
        self.states_client
            .get_asserted_states()
            .map_err(|e| EdenFsError::from(anyhow::Error::from(e)))
    }

    pub fn is_state_asserted(&self, state: &str) -> Result<bool> {
        self.states_client
            .is_state_asserted(state)
            .map_err(|e| EdenFsError::from(anyhow::Error::from(e)))
    }

    // Takes a list of known states and filters them based on the currently desired states
    fn filter_states(&self, states: &[String], known_states: &HashSet<String>) -> HashSet<String> {
        let mut output = HashSet::new();
        for state in states {
            if known_states.contains(state) {
                output.insert(state.clone());
            }
        }
        output
    }

    // Takes a stream of ChangesSinceV2Result, and returns a stream of Changes.
    // The Changes will be the file changes from the input stream as ChangesSinceV2Results split by ChangeEvents
    pub async fn stream_changes_since_with_states_wrapper<'a>(
        &'a self,
        inner_stream: BoxStream<'a, Result<ChangesSinceV2Result>>,
        states: &'a [String],
        known_asserted_states: Option<&HashSet<String>>,
    ) -> Result<BoxStream<'a, Result<Changes>>> {
        self.instrument("stream_changes_since_with_states_wrapper");
        self._stream_changes_since_with_states_wrapper(inner_stream, states, known_asserted_states)
            .await
    }

    // Takes a stream of ChangesSinceV2Result, and returns a stream of Changes.
    // The Changes will be the file changes from the input stream as ChangesSinceV2Results split by ChangeEvents
    async fn _stream_changes_since_with_states_wrapper<'a>(
        &'a self,
        inner_stream: BoxStream<'a, Result<ChangesSinceV2Result>>,
        states: &'a [String],
        known_asserted_states: Option<&HashSet<String>>,
    ) -> Result<BoxStream<'a, Result<Changes>>> {
        // On init, check for asserted states. Set asserted_states to that value.
        // This may happen in the case where the stream was started after a state has been
        // asserted, or if a state was entered on the first result from the stream.
        let asserted_states = match known_asserted_states {
            Some(known_states) => self.filter_states(states, known_states),
            None => self.which_states_asserted(states)?,
        };

        let state = if asserted_states.is_empty() {
            IsStateCurrentlyAsserted::NotAsserted
        } else {
            IsStateCurrentlyAsserted::StateAsserted
        };

        let state_data = StreamChangesSinceWithStatesData {
            inner_stream: inner_stream.boxed(),
            state,
            asserted_states,
            position: JournalPosition::default(),
        };

        let stream = stream::unfold(state_data, move |mut state_data| async move {
            match state_data.state {
                IsStateCurrentlyAsserted::NotAsserted => {
                    let next_result = state_data.inner_stream.as_mut().next().await?;
                    match next_result {
                        Ok(inner_result) => {
                            state_data.position = inner_result.to_position.clone();

                            let (nested, new_state) = self.split_inner_result(
                                inner_result,
                                states,
                                &state_data.state,
                                &mut state_data.asserted_states,
                            );

                            state_data.state = new_state;
                            Some((nested, state_data))
                        }
                        Err(e) => {
                            // Pass through the error
                            Some((stream::iter(std::iter::once(Err(e))).boxed(), state_data))
                        }
                    }
                }
                IsStateCurrentlyAsserted::StateAsserted => {
                    let timer = tokio::time::interval(Duration::from_secs(1));
                    tokio::pin!(timer);
                    loop {
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
                                        let mut change_events = Vec::new();
                                        for difference in left_states {
                                            tracing::debug!("Found deasserted state during timer check: {:?}", difference);
                                            change_events.push(ChangeEvent {
                                                event_type: StateChange::Left,
                                                state: difference.to_string(),
                                                position: state_data.position.clone(),
                                            });
                                            state_data.asserted_states.remove(&difference);
                                        }

                                        if asserted_states.is_empty() {
                                            state_data.state = IsStateCurrentlyAsserted::NotAsserted;
                                        }
                                        let results = stream::iter(change_events.into_iter().map(|change_event| Ok(Changes::ChangeEvent(change_event)))).boxed();
                                        return Some((results, state_data));
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
                                            Ok(inner_result) => {
                                                state_data.position = inner_result.to_position.clone();
                                                let (nested, new_state) = self.split_inner_result(inner_result, states, &state_data.state, &mut state_data.asserted_states);
                                                state_data.state = new_state;
                                                return Some((nested, state_data))
                                            }
                                            Err(e) => {
                                                // Pass through the error. The stream will be terminated
                                                // inside the subscription after surfacing an error.
                                                tracing::error!("error while checking states {:?}", e);
                                                return Some((stream::iter(std::iter::once(Err(e))).boxed(), state_data))
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
        Ok(stream.flatten().boxed())
    }

    pub fn split_inner_result(
        &'_ self,
        inner_result: ChangesSinceV2Result,
        states: &[String],
        current_state: &IsStateCurrentlyAsserted,
        asserted_states: &mut HashSet<String>,
    ) -> (BoxStream<'_, Result<Changes>>, IsStateCurrentlyAsserted) {
        let changes_with_events = self.insert_change_events(states, asserted_states, inner_result);

        let output_state = match current_state {
            IsStateCurrentlyAsserted::NotAsserted => {
                if !asserted_states.is_empty() {
                    IsStateCurrentlyAsserted::StateAsserted
                } else {
                    IsStateCurrentlyAsserted::NotAsserted
                }
            }
            IsStateCurrentlyAsserted::StateAsserted => {
                if asserted_states.is_empty() {
                    IsStateCurrentlyAsserted::NotAsserted
                } else {
                    IsStateCurrentlyAsserted::StateAsserted
                }
            }
        };

        let nested = stream::iter(changes_with_events.into_iter().map(Ok)).boxed();
        (nested, output_state)
    }

    /// Like [`stream_changes_since_with_states_wrapper`], but defers changes as long as any state in
    /// `states` is asserted.
    pub async fn stream_changes_since_with_deferral<'a>(
        &'a self,
        inner_stream: BoxStream<'a, Result<ChangesSinceV2Result>>,
        states: &'a [String],
        known_asserted_states: Option<&HashSet<String>>,
    ) -> Result<BoxStream<'a, Result<Changes>>> {
        self.instrument("stream_changes_since_with_deferral");

        let mut deferred_changes: Vec<ChangeNotification> = Vec::new();
        let mut asserted_states = HashSet::new();

        let stream = self
            ._stream_changes_since_with_states_wrapper(inner_stream, states, known_asserted_states)
            .await?;
        let stream = stream.flat_map(move |from_stream| match from_stream {
            Ok(changes) => match changes {
                Changes::ChangeEvent(ref change_event) => {
                    match change_event.event_type {
                        StateChange::Entered => asserted_states.insert(change_event.state.clone()),
                        StateChange::Left => asserted_states.remove(&change_event.state),
                    };
                    if asserted_states.is_empty() && !deferred_changes.is_empty() {
                        let deferred_changes_since =
                            Ok(Changes::ChangesSince(ChangesSinceV2Result {
                                to_position: change_event.position.clone(),
                                changes: std::mem::take(&mut deferred_changes),
                            }));
                        stream::iter([Ok(changes), deferred_changes_since]).boxed()
                    } else {
                        stream::once(async { Ok(changes) }).boxed()
                    }
                }
                Changes::ChangesSince(changes_since) => {
                    if asserted_states.is_empty() {
                        stream::once(async { Ok(Changes::ChangesSince(changes_since)) }).boxed()
                    } else {
                        deferred_changes.extend(changes_since.changes);
                        stream::empty().boxed()
                    }
                }
            },
            Err(_) => stream::once(async { from_stream }).boxed(),
        });
        Ok(stream.boxed())
    }

    fn which_states_asserted(&self, states: &[String]) -> Result<HashSet<String>> {
        self.states_client
            .which_states_asserted(states)
            .map_err(|e| EdenFsError::from(anyhow::Error::from(e)))
    }

    fn to_change_event(
        &self,
        states: &[String],
        change: &ChangeNotification,
        position: &JournalPosition,
    ) -> Option<ChangeEvent> {
        if let ChangeNotification::StateChange(state_change) = change {
            match state_change {
                StateChangeNotification::StateEntered(entered) => {
                    let state_name = unsanitize_lock_name(&entered.name);
                    if states.contains(&state_name) {
                        return Some(ChangeEvent {
                            event_type: StateChange::Entered,
                            state: state_name,
                            position: position.clone(),
                        });
                    }
                }
                StateChangeNotification::StateLeft(left) => {
                    let state_name = unsanitize_lock_name(&left.name);
                    if states.contains(&state_name) {
                        return Some(ChangeEvent {
                            event_type: StateChange::Left,
                            state: state_name,
                            position: position.clone(),
                        });
                    }
                }
            }
        }
        None
    }

    fn insert_change_events(
        &self,
        states: &[String],
        asserted_states: &mut HashSet<String>,
        changes_since: ChangesSinceV2Result,
    ) -> Vec<Changes> {
        if changes_since.changes.iter().all(|change| {
            self.to_change_event(states, change, &changes_since.to_position)
                .is_none()
        }) {
            // Just an optimization: Most of the time, `changes_since` will not contain any state
            // changes and we can return it as-is. The code below would unnecessarily build a new
            // `Vec<ChangeNotification>`.
            return vec![Changes::ChangesSince(changes_since)];
        }

        let mut result = Vec::new();
        let key = |change: &ChangeNotification| {
            self.to_change_event(states, change, &changes_since.to_position)
        };

        // We put all elements from `changes` into groups. For each value `c` in `changes`, we
        // compare `key(c)` with `key(pred)`, where `pred` is the predecessor of `c` in `changes`.
        //
        // - If `key(c)` is equal to `key(pred)`, they go into the same group.
        // - Otherwise, if they are not equal, or `c` is the first element, `c` goes into a new group.
        //
        // This means that:
        // - All consecutive items that are not a `ChangeEvent` are grouped together (`key` yields
        //   `None`).
        // - Items that correspond to a `StateChange` land in a group of their own.
        let groups = changes_since.changes.into_iter().chunk_by(key);

        for (key, group) in &groups {
            let to_position = changes_since.to_position.clone();
            let changes: Vec<ChangeNotification> = group.collect();

            match key {
                None => {
                    result.push(Changes::ChangesSince(ChangesSinceV2Result {
                        to_position,
                        changes,
                    }));
                }
                Some(change_event) => {
                    match change_event.event_type {
                        StateChange::Entered => {
                            asserted_states.insert(change_event.state.clone());
                        }
                        StateChange::Left => {
                            asserted_states.remove(&change_event.state);
                        }
                    }
                    result.push(Changes::ChangeEvent(change_event))
                }
            }
        }
        result
    }
}

pub enum IsStateCurrentlyAsserted {
    NotAsserted,
    StateAsserted,
}

struct StreamChangesSinceWithStatesData<'a> {
    inner_stream: BoxStream<'a, Result<ChangesSinceV2Result>>,
    state: IsStateCurrentlyAsserted,
    asserted_states: HashSet<String>,
    position: JournalPosition,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ChangeEvent {
    pub event_type: StateChange,
    pub state: String,
    pub position: JournalPosition,
}

impl fmt::Display for ChangeEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} at {}", self.event_type, self.state, self.position)
    }
}

#[derive(Debug, PartialEq)]
pub enum Changes {
    ChangeEvent(ChangeEvent),
    ChangesSince(ChangesSinceV2Result),
}

impl AsRef<JournalPosition> for Changes {
    fn as_ref(&self) -> &JournalPosition {
        match self {
            Changes::ChangeEvent(change_event) => &change_event.position,
            Changes::ChangesSince(changes_since_v2_result) => &changes_since_v2_result.to_position,
        }
    }
}

impl fmt::Display for Changes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Changes::ChangeEvent(event) => write!(f, "{}", event),
            Changes::ChangesSince(changes) => write!(f, "{}", changes),
        }
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
    use edenfs_asserted_states_client::*;
    use edenfs_client::changes_since::*;
    use edenfs_client::types::Dtype;
    use util::lock::ContentLock;

    use crate::*;

    #[test]
    fn test_enter_state() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount");
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
        let state = "test_state1";
        let _result = client.enter_state(state)?;
        let check_state = client.is_state_asserted(state)?;
        assert!(check_state);
        Ok(())
    }

    #[test]
    fn test_state_leave() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount1");
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
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
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
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
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
        let asserted_states = client.get_asserted_states()?;
        assert!(asserted_states.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_asserted_states() -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount3");
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
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
        let client1 = StreamingChangesClient::new(&mount_point1, "test".to_string())?;
        let client2 = StreamingChangesClient::new(&mount_point2, "test".to_string())?;
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
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
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
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;
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
    fn test_insert_change_events() -> anyhow::Result<()> {
        let bytify = |str: &str| str.as_bytes().to_vec();

        let mount_point = std::env::temp_dir().join("test_mount10");
        let client = StreamingChangesClient::new(&mount_point, "test".to_string())?;

        let journal_pos = JournalPosition {
            mount_generation: 0,
            sequence_number: 0,
            snapshot_hash: vec![0, 1, 2, 3, 4],
        };

        let tracked_states = vec!["test-state1".to_string(), "test-state2".to_string()];
        let mut asserted_states = HashSet::from(["test-state1".to_string()]);

        use ChangeNotification::SmallChange as SC;
        use ChangeNotification::StateChange as STC;
        use edenfs_client::changes_since::SmallChangeNotification as SCN;
        use edenfs_client::changes_since::StateChangeNotification as STCN;

        let changes_since = ChangesSinceV2Result {
            to_position: journal_pos.clone(),
            changes: vec![
                SC(SCN::Added(Added {
                    file_type: Dtype::Regular,
                    path: bytify("file1.txt"),
                })),
                SC(SCN::Added(Added {
                    file_type: Dtype::Regular,
                    path: bytify("file2.txt"),
                })),
                STC(STCN::StateLeft(StateLeft {
                    name: "test-state1".into(),
                })),
                SC(SCN::Added(Added {
                    file_type: Dtype::Regular,
                    path: bytify("file3.txt"),
                })),
                SC(SCN::Added(Added {
                    file_type: Dtype::Regular,
                    path: bytify("file4.txt"),
                })),
                STC(STCN::StateEntered(StateEntered {
                    name: "test-state2".into(),
                })),
                SC(SCN::Removed(Removed {
                    file_type: Dtype::Regular,
                    path: bytify("file5.txt"),
                })),
            ],
        };

        let result =
            client.insert_change_events(&tracked_states, &mut asserted_states, changes_since);

        assert_eq!(
            result,
            vec![
                Changes::ChangesSince(ChangesSinceV2Result {
                    to_position: journal_pos.clone(),
                    changes: vec![
                        SC(SCN::Added(Added {
                            file_type: Dtype::Regular,
                            path: bytify("file1.txt"),
                        })),
                        SC(SCN::Added(Added {
                            file_type: Dtype::Regular,
                            path: bytify("file2.txt"),
                        })),
                    ]
                }),
                Changes::ChangeEvent(ChangeEvent {
                    event_type: StateChange::Left,
                    state: "test-state1".to_string(),
                    position: journal_pos.clone()
                }),
                Changes::ChangesSince(ChangesSinceV2Result {
                    to_position: journal_pos.clone(),
                    changes: vec![
                        SC(SCN::Added(Added {
                            file_type: Dtype::Regular,
                            path: bytify("file3.txt"),
                        })),
                        SC(SCN::Added(Added {
                            file_type: Dtype::Regular,
                            path: bytify("file4.txt"),
                        })),
                    ]
                }),
                Changes::ChangeEvent(ChangeEvent {
                    event_type: StateChange::Entered,
                    state: "test-state2".to_string(),
                    position: journal_pos.clone()
                }),
                Changes::ChangesSince(ChangesSinceV2Result {
                    to_position: journal_pos.clone(),
                    changes: vec![SC(SCN::Removed(Removed {
                        file_type: Dtype::Regular,
                        path: bytify("file5.txt"),
                    })),]
                }),
            ]
        );

        assert_eq!(asserted_states, HashSet::from(["test-state2".to_string()]));

        Ok(())
    }
}
