/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use repolock::RepoLocker;
use serde::Deserialize;
use types::RepoPathBuf;
use watchman_client::prelude::*;

use super::treestate::WatchmanTreeStateRead;
use super::treestate::WatchmanTreeStateWrite;
use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filechangedetector::FileChangeResult;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::PendingChangeResult;

query_result_type! {
    pub struct StatusQuery {
        name: NameField,
        exists: ExistsField,
    }
}

pub struct WatchmanState {
    treestate_needs_check: HashSet<RepoPathBuf>,
    clock: Option<Clock>,
    treestate_errors: Vec<Error>,
    timeout: Option<std::time::Duration>,
}

impl WatchmanState {
    pub fn new(config: &dyn Config, mut treestate: impl WatchmanTreeStateRead) -> Result<Self> {
        let (needs_check, errors): (Vec<_>, Vec<_>) = treestate
            .list_needs_check()?
            .into_iter()
            .partition(Result::is_ok);

        let needs_check = needs_check
            .into_iter()
            .map(Result::unwrap)
            .collect::<HashSet<_>>();
        let errors = errors.into_iter().map(Result::unwrap_err).collect();

        Ok(WatchmanState {
            treestate_needs_check: needs_check,
            clock: treestate.get_clock()?,
            treestate_errors: errors,
            timeout: config
                .get_opt::<u64>("fsmonitor", "timeout")?
                .map(std::time::Duration::from_millis),
        })
    }

    pub fn get_clock(&self) -> Option<Clock> {
        self.clock.clone()
    }

    pub fn sync_timeout(&self) -> SyncTimeout {
        match self.timeout {
            None => SyncTimeout::Default,
            Some(d) => SyncTimeout::Duration(d),
        }
    }

    pub fn merge(
        self,
        result: QueryResult<StatusQuery>,
        mut file_change_detector: impl FileChangeDetectorTrait + 'static,
    ) -> Result<WatchmanPendingChanges> {
        let (needs_check, errors): (Vec<_>, Vec<_>) = result
            .files
            .unwrap_or_default()
            .into_iter()
            .map(|query| RepoPathBuf::try_from(query.name.into_inner()))
            .partition(Result::is_ok);

        let mut needs_check = needs_check
            .into_iter()
            .map(Result::unwrap)
            .collect::<HashSet<_>>();
        needs_check.extend(self.treestate_needs_check.iter().cloned());

        let mut errors = errors
            .into_iter()
            .map(|e| anyhow!(e.unwrap_err()))
            .collect::<Vec<_>>();
        errors.extend(self.treestate_errors.into_iter());

        let mut needs_clear: Vec<RepoPathBuf> = vec![];
        let mut needs_mark: Vec<RepoPathBuf> = vec![];
        let mut pending_changes = needs_check
            .into_iter()
            .filter_map(|path| match file_change_detector.has_changed(&path) {
                Ok(FileChangeResult::Yes(change)) => {
                    needs_mark.push(path);
                    Some(Ok(PendingChangeResult::File(change)))
                }
                Ok(FileChangeResult::No) => {
                    if self.treestate_needs_check.contains(&path) {
                        needs_clear.push(path);
                    }
                    None
                }
                Err(e) => Some(Err(e)),
                _ => None,
            })
            .collect::<Vec<_>>();
        pending_changes.extend(errors.into_iter().map(Err));

        for result in file_change_detector.resolve_maybes() {
            match result {
                Ok(ResolvedFileChangeResult::Yes(change)) => {
                    needs_mark.push(change.get_path().clone());
                    pending_changes.push(Ok(PendingChangeResult::File(change)));
                }
                Ok(ResolvedFileChangeResult::No(path)) => {
                    if self.treestate_needs_check.contains(&path) {
                        needs_clear.push(path);
                    }
                }
                Err(e) => pending_changes.push(Err(e)),
            }
        }

        Ok(WatchmanPendingChanges {
            pending_changes,
            needs_clear,
            needs_mark,
            clock: result.clock,
        })
    }
}

pub struct WatchmanPendingChanges {
    pending_changes: Vec<Result<PendingChangeResult>>,
    needs_clear: Vec<RepoPathBuf>,
    needs_mark: Vec<RepoPathBuf>,
    clock: Clock,
}

impl WatchmanPendingChanges {
    pub fn persist(
        &mut self,
        mut treestate: impl WatchmanTreeStateWrite,
        should_update_clock: bool,
        locker: &RepoLocker,
    ) -> Result<()> {
        let mut wrote = false;
        for path in self.needs_clear.iter() {
            match treestate.clear_needs_check(&path) {
                Ok(v) => wrote |= v,
                Err(e) =>
                // We can still build a valid result if we fail to clear the
                // needs check flag. Propagate the error to the caller but allow
                // the persist to continue.
                {
                    self.pending_changes.push(Err(e))
                }
            }
        }

        for path in self.needs_mark.iter() {
            wrote |= treestate.mark_needs_check(&path)?;
        }

        // If the treestate is already dirty, we're going to write it anyway, so let's go ahead and
        // update the clock while we're at it.
        if should_update_clock || wrote {
            treestate.set_clock(self.clock.clone())?;
        }

        treestate.flush(locker)
    }
}

impl IntoIterator for WatchmanPendingChanges {
    type Item = Result<PendingChangeResult>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.pending_changes.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::HashSet;

    use anyhow::Result;
    use repolock::RepoLocker;
    use types::RepoPath;
    use types::RepoPathBuf;
    use watchman_client::prelude::*;

    use super::super::state::StatusQuery;
    use super::super::state::WatchmanState;
    use super::super::treestate::WatchmanTreeStateRead;
    use super::super::treestate::WatchmanTreeStateWrite;
    use crate::filechangedetector::FileChangeDetectorTrait;
    use crate::filechangedetector::FileChangeResult;
    use crate::filechangedetector::ResolvedFileChangeResult;
    use crate::filesystem::ChangeType;
    use crate::filesystem::PendingChangeResult;

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

    impl WatchmanTreeStateWrite for WatchmanStateTestTreeState {
        fn mark_needs_check(&mut self, _path: &RepoPathBuf) -> Result<bool> {
            Ok(true)
        }

        fn clear_needs_check(&mut self, _path: &RepoPathBuf) -> Result<bool> {
            Ok(true)
        }

        fn set_clock(&mut self, _clock: Clock) -> Result<()> {
            Ok(())
        }

        fn flush(self, _locker: &RepoLocker) -> Result<()> {
            Ok(())
        }
    }

    struct WatchmanStateTestFileChangeDetector {
        changed_files: HashSet<RepoPathBuf>,
        deleted_files: HashSet<RepoPathBuf>,
    }

    impl FileChangeDetectorTrait for WatchmanStateTestFileChangeDetector {
        fn has_changed(&mut self, path: &RepoPath) -> Result<FileChangeResult> {
            if self.changed_files.contains(path) {
                return Ok(FileChangeResult::Yes(ChangeType::Changed(path.to_owned())));
            }

            if self.deleted_files.contains(path) {
                return Ok(FileChangeResult::Yes(ChangeType::Deleted(path.to_owned())));
            }

            Ok(FileChangeResult::No)
        }

        fn resolve_maybes(
            &self,
        ) -> Box<dyn Iterator<Item = Result<ResolvedFileChangeResult>> + Send> {
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

    #[test]
    fn pending_changes_test() {
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
        let state = WatchmanState::new(&BTreeMap::<&str, &str>::new(), test.treestate()).unwrap();

        let pending_changes = state
            .merge(test.query_result(), test.file_change_detector())
            .unwrap();

        assert_eq!(
            to_string(test.expected_pending_changes()),
            to_string(pending_changes.into_iter()),
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
