/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use parking_lot::Mutex;
use repolock::RepoLocker;
use treestate::dirstate;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::metadata::Metadata;
use treestate::serialization::Serializable;
use treestate::treestate::TreeState;
use treestate::ErrorKind;
use types::RepoPathBuf;
use watchman_client::prelude::*;

pub trait WatchmanTreeStateWrite {
    fn mark_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool>;

    fn clear_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool>;

    fn set_clock(&mut self, clock: Clock) -> Result<()>;

    fn flush(self, locker: &RepoLocker) -> Result<()>;
}

pub trait WatchmanTreeStateRead {
    fn list_needs_check(&mut self) -> Result<Vec<Result<RepoPathBuf>>>;

    fn get_clock(&self) -> Result<Option<Clock>>;
}

pub struct WatchmanTreeState<'a> {
    pub treestate: Arc<Mutex<TreeState>>,
    pub root: &'a Path,
}

impl WatchmanTreeStateWrite for WatchmanTreeState<'_> {
    fn mark_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool> {
        let mut treestate = self.treestate.lock();

        let state = treestate.get(path)?;
        let filestate = match state {
            Some(filestate) => {
                let filestate = filestate.clone();
                if filestate.state.intersects(StateFlags::NEED_CHECK) {
                    // It's already marked need_check, so return early so we don't mutate the
                    // treestate.
                    return Ok(false);
                }
                FileStateV2 {
                    state: filestate.state | StateFlags::NEED_CHECK,
                    ..filestate
                }
            }
            // The file is currently untracked
            None => FileStateV2 {
                state: StateFlags::NEED_CHECK,
                mode: 0o666,
                size: -1,
                mtime: -1,
                copied: None,
            },
        };
        treestate.insert(path, &filestate)?;
        Ok(true)
    }

    fn clear_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool> {
        let mut treestate = self.treestate.lock();

        let state = treestate.get(path)?;
        if let Some(filestate) = state {
            let filestate = filestate.clone();
            if !filestate.state.intersects(StateFlags::NEED_CHECK) {
                // It's already clear.
                return Ok(false);
            }
            let filestate = FileStateV2 {
                state: filestate.state & !StateFlags::NEED_CHECK,
                ..filestate
            };
            treestate.insert(path, &filestate)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn set_clock(&mut self, clock: Clock) -> Result<()> {
        let mut treestate = self.treestate.lock();

        let clock_string = match clock {
            Clock::Spec(ClockSpec::StringClock(string)) => Ok(string),
            clock => Err(anyhow!(
                "Watchman implementation only handles opaque string type. Got the following clock instead: {:?}",
                clock
            )),
        }?;

        let mut metadata_buf = treestate.get_metadata();
        let mut metadata = Metadata::deserialize(&mut metadata_buf)?;
        metadata.0.insert("clock".to_string(), clock_string);
        let mut metadata_buf = vec![];
        metadata.serialize(&mut metadata_buf)?;
        treestate.set_metadata(&metadata_buf);

        Ok(())
    }

    fn flush(self, locker: &RepoLocker) -> Result<()> {
        match dirstate::flush(&self.root, &mut self.treestate.lock(), locker) {
            Ok(()) => Ok(()),
            // If the dirstate was changed before we flushed, that's ok. Let the other write win
            // since writes during status are just optimizations.
            Err(e) => match e.downcast_ref::<ErrorKind>() {
                Some(e) if *e == ErrorKind::TreestateOutOfDate => Ok(()),
                _ => Err(e),
            },
        }
    }
}

impl WatchmanTreeStateRead for WatchmanTreeState<'_> {
    fn list_needs_check(&mut self) -> Result<Vec<Result<RepoPathBuf>>> {
        Ok(self
            .treestate
            .lock()
            .visit_by_state(StateFlags::NEED_CHECK)?
            .into_iter()
            .map(|(path, _state)| RepoPathBuf::from_utf8(path).map_err(|e| anyhow!(e)))
            .collect())
    }

    fn get_clock(&self) -> Result<Option<Clock>> {
        let treestate = self.treestate.lock();

        let mut metadata_buf = treestate.get_metadata();
        let metadata = Metadata::deserialize(&mut metadata_buf)?;
        Ok(metadata
            .0
            .get(&"clock".to_string())
            .map(|clock| Clock::Spec(ClockSpec::StringClock(clock.clone()))))
    }
}
