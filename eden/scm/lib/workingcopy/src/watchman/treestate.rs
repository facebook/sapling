/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use parking_lot::MutexGuard;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::metadata::Metadata;
use treestate::serialization::Serializable;
use treestate::treestate::TreeState;
use types::RepoPathBuf;
use watchman_client::prelude::*;

pub trait WatchmanTreeStateWrite {
    fn mark_needs_check(&mut self, path: &RepoPathBuf) -> Result<()>;

    fn clear_needs_check(&mut self, path: &RepoPathBuf) -> Result<()>;

    fn set_clock(&mut self, clock: Clock) -> Result<()>;
}

pub struct WatchmanTreeState<'a> {
    pub treestate: MutexGuard<'a, TreeState>,
}

impl WatchmanTreeStateWrite for WatchmanTreeState<'_> {
    fn mark_needs_check(&mut self, path: &RepoPathBuf) -> Result<()> {
        let state = self.treestate.get(path)?;
        let filestate = match state {
            Some(filestate) => {
                let filestate = filestate.clone();
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
        self.treestate.insert(path, &filestate)
    }

    fn clear_needs_check(&mut self, path: &RepoPathBuf) -> Result<()> {
        let state = self.treestate.get(path)?;
        if let Some(filestate) = state {
            let filestate = filestate.clone();
            let filestate = FileStateV2 {
                state: filestate.state & !StateFlags::NEED_CHECK,
                ..filestate
            };
            self.treestate.insert(path, &filestate)?;
        }
        Ok(())
    }

    fn set_clock(&mut self, clock: Clock) -> Result<()> {
        let clock_string = match clock {
            Clock::Spec(ClockSpec::StringClock(string)) => Ok(string),
            clock => Err(anyhow!(
                "Watchman implementation only handles opaque string type. Got the following clock instead: {:?}",
                clock
            )),
        }?;

        let mut metadata_buf = self.treestate.get_metadata();
        let mut metadata = Metadata::deserialize(&mut metadata_buf)?;
        metadata.0.insert("clock".to_string(), clock_string);
        let mut metadata_buf = vec![];
        metadata.serialize(&mut metadata_buf)?;
        self.treestate.set_metadata(&metadata_buf);

        Ok(())
    }
}
