/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use serde::Deserialize;
use watchman_client::prelude::*;

use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filesystem::PendingChangeResult;

use super::treestate::WatchmanTreeStateRead;
use super::treestate::WatchmanTreeStateWrite;

query_result_type! {
    pub struct StatusQuery {
        name: NameField,
        exists: ExistsField,
    }
}

pub struct WatchmanState {}

impl WatchmanState {
    pub fn new(
        mut _treestate: impl WatchmanTreeStateRead,
        mut _file_change_detector: impl FileChangeDetectorTrait,
    ) -> Self {
        WatchmanState {}
    }

    pub fn get_clock(&self) -> Option<Clock> {
        None
    }

    pub fn merge(&mut self, _result: QueryResult<StatusQuery>) {}

    pub fn persist(&self, mut _treestate: impl WatchmanTreeStateWrite) -> Result<()> {
        todo!();
    }

    pub fn into_pending_changes(self) -> impl Iterator<Item = Result<PendingChangeResult>> {
        vec![].into_iter()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use anyhow::Result;
    use types::RepoPathBuf;

    use watchman_client::prelude::*;

    use crate::filechangedetector::FileChangeDetectorTrait;
    use crate::filechangedetector::FileChangeResult;
    use crate::filesystem::ChangeType;
    use crate::filesystem::PendingChangeResult;
    use crate::watchman::state::StatusQuery;
    use crate::watchman::state::WatchmanState;
    use crate::watchman::treestate::WatchmanTreeStateRead;

    #[derive(Clone)]
    enum Event {
        Changed,
        Deleted,
        Reverted,
        Nothing,
    }

    #[derive(Clone)]
    enum InitialState {
        Changed,
        Deleted,
        Clean,
    }

    struct WatchmanStateTestTreeState {
        needs_check: Vec<RepoPathBuf>,
    }

    impl WatchmanTreeStateRead for WatchmanStateTestTreeState {
        fn list_needs_check(&mut self) -> Result<Vec<Result<RepoPathBuf>>> {
            Ok(self
                .needs_check
                .iter()
                .cloned()
                .map(|path| Ok(path))
                .collect())
        }

        fn get_clock(&self) -> Result<Option<Clock>> {
            Ok(None)
        }
    }

    struct WatchmanStateTestFileChangeDetector {
        changed_files: HashSet<RepoPathBuf>,
        deleted_files: HashSet<RepoPathBuf>,
    }

    impl FileChangeDetectorTrait for WatchmanStateTestFileChangeDetector {
        fn has_changed(&mut self, path: &RepoPathBuf) -> Result<FileChangeResult> {
            if self.changed_files.contains(path) {
                return Ok(FileChangeResult::Yes(ChangeType::Changed(path.clone())));
            }

            if self.deleted_files.contains(path) {
                return Ok(FileChangeResult::Yes(ChangeType::Deleted(path.clone())));
            }

            Ok(FileChangeResult::No)
        }

        fn resolve_maybes(&self) -> Box<dyn Iterator<Item = Result<ChangeType>> + Send> {
            Box::new(vec![].into_iter())
        }
    }

    struct WatchmanStateTest {
        events: Vec<(RepoPathBuf, InitialState, Event)>,
    }

    impl WatchmanStateTest {
        fn new(events: Vec<(InitialState, Event)>) -> Self {
            let events = events
                .into_iter()
                .enumerate()
                .map(|(i, (state, event))| {
                    (
                        RepoPathBuf::from_string(format!("file{}.txt", i)).expect("Path is valid"),
                        state,
                        event,
                    )
                })
                .collect();
            WatchmanStateTest { events }
        }

        fn query_result(&self) -> QueryResult<StatusQuery> {
            QueryResult {
                version: "1".to_string(),
                is_fresh_instance: false,
                files: Some(
                    self.events
                        .iter()
                        .filter_map(|(path, _state, event)| match event {
                            Event::Changed | Event::Reverted => Some(StatusQuery {
                                name: NameField::new(path.to_path()),
                                exists: ExistsField::new(true),
                            }),
                            Event::Deleted => Some(StatusQuery {
                                name: NameField::new(path.to_path()),
                                exists: ExistsField::new(false),
                            }),
                            Event::Nothing => None,
                        })
                        .collect(),
                ),
                clock: Clock::Spec(ClockSpec::default()),
                state_enter: None,
                state_leave: None,
                state_metadata: None,
                saved_state_info: None,
                debug: None,
            }
        }

        fn treestate(&self) -> WatchmanStateTestTreeState {
            WatchmanStateTestTreeState {
                needs_check: self
                    .events
                    .iter()
                    .cloned()
                    .filter_map(|(path, state, _event)| match state {
                        InitialState::Changed | InitialState::Deleted => Some(path),
                        _ => None,
                    })
                    .collect(),
            }
        }

        fn file_change_detector(&self) -> WatchmanStateTestFileChangeDetector {
            let changed_files = self
                .events
                .iter()
                .cloned()
                .filter_map(|(path, state, event)| match (state, event) {
                    (_, Event::Changed) | (InitialState::Changed, Event::Nothing) => Some(path),
                    (_, _) => None,
                })
                .collect();

            let deleted_files = self
                .events
                .iter()
                .cloned()
                .filter_map(|(path, state, event)| match (state, event) {
                    (_, Event::Deleted) | (InitialState::Deleted, Event::Nothing) => Some(path),
                    (_, _) => None,
                })
                .collect();

            WatchmanStateTestFileChangeDetector {
                changed_files,
                deleted_files,
            }
        }

        fn expected_pending_changes(self) -> impl Iterator<Item = Result<PendingChangeResult>> {
            self.events
                .into_iter()
                .filter_map(|(path, state, event)| match (state, event) {
                    (_, Event::Changed) | (InitialState::Changed, Event::Nothing) => {
                        Some(Ok(PendingChangeResult::File(ChangeType::Changed(path))))
                    }
                    (_, Event::Deleted) | (InitialState::Deleted, Event::Nothing) => {
                        Some(Ok(PendingChangeResult::File(ChangeType::Deleted(path))))
                    }
                    (_, _) => None,
                })
        }
    }

    #[ignore = "Merge not implemented yet"]
    #[test]
    fn merge_test() {
        // The idea of this test is to test every possible combination of
        // initial states (i.e. persisted watchman state) and valid events
        // (i.e. watchman updates) to ensure merge handles as possible
        // combinations correctly.
        let events = vec![
            (InitialState::Changed, Event::Changed),
            (InitialState::Changed, Event::Reverted),
            (InitialState::Changed, Event::Deleted),
            (InitialState::Changed, Event::Nothing),
            (InitialState::Deleted, Event::Changed),
            (InitialState::Deleted, Event::Reverted),
            // Note that it's possible to receive a delete event even though
            // the initial state is deleted because the file could have been
            // re-created since the last query and deleted again. Watchman
            // will still propagate the delete event in this case.
            (InitialState::Deleted, Event::Deleted),
            (InitialState::Deleted, Event::Nothing),
            (InitialState::Clean, Event::Changed),
            (InitialState::Clean, Event::Deleted),
        ];

        let test = WatchmanStateTest::new(events);
        let mut state = WatchmanState::new(test.treestate(), test.file_change_detector());

        state.merge(test.query_result());

        assert_eq!(
            to_string(test.expected_pending_changes()),
            to_string(state.into_pending_changes()),
        );
    }

    fn to_string(results: impl Iterator<Item = Result<PendingChangeResult>>) -> String {
        let mut results = results.map(Result::unwrap).collect::<Vec<_>>();
        results.sort_by(|a, b| match (a, b) {
            (PendingChangeResult::File(a), PendingChangeResult::File(b)) => match (a, b) {
                (
                    ChangeType::Changed(a) | ChangeType::Deleted(a),
                    ChangeType::Changed(b) | ChangeType::Deleted(b),
                ) => a.cmp(b),
            },
            _ => panic!("Unexpected pending change result"),
        });
        serde_json::to_string(&results).unwrap()
    }
}
