/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use parking_lot::Mutex;
use treestate::dirstate::Dirstate;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::metadata::Metadata;
use treestate::serialization::Serializable;
use treestate::treestate::TreeState;
use types::RepoPathBuf;
use watchman_client::prelude::*;

pub trait WatchmanTreeStateWrite {
    fn mark_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool>;

    fn clear_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool>;

    fn set_clock(&mut self, clock: Clock) -> Result<()>;

    fn flush(self, config: &dyn Config) -> Result<()>;
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

    fn flush(self, config: &dyn Config) -> Result<()> {
        let mut treestate = self.treestate.lock();
        if treestate.dirty() {
            tracing::debug!("flushing dirty treestate");
            let id = identity::must_sniff_dir(self.root)?;
            let dot_dir = self.root.join(id.dot_dir());
            let dirstate_path = dot_dir.join("dirstate");

            let _locked = repolock::lock_working_copy(config, &dot_dir)?;

            let dirstate_input = util::file::read(&dirstate_path).map_err(|e| anyhow!(e))?;
            let mut dirstate = Dirstate::deserialize(&mut dirstate_input.as_slice())?;
            let treestate_fields = dirstate.tree_state.as_mut().ok_or_else(|| {
                anyhow!(
                    "Unable to flush treestate because dirstate is missing required treestate fields"
                )
            })?;

            let root_id = treestate.flush()?;
            treestate_fields.tree_root_id = root_id;

            let mut dirstate_output: Vec<u8> = Vec::new();
            dirstate.serialize(&mut dirstate_output).unwrap();
            util::file::atomic_write(&dirstate_path, |file| file.write_all(&dirstate_output))
                .map_err(|e| anyhow!(e))
                .map(|_| ())
        } else {
            tracing::debug!("skipping treestate flush - it is not dirty");
            Ok(())
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
