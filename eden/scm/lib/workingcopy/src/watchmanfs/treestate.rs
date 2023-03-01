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
use pathmatcher::Matcher;
use repolock::RepoLocker;
use treestate::dirstate;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::metadata::Metadata;
use treestate::serialization::Serializable;
use treestate::treestate::TreeState;
use treestate::ErrorKind;
use types::path::ParseError;
use types::RepoPathBuf;
use watchman_client::prelude::*;

use crate::util::walk_treestate;

pub trait WatchmanTreeStateWrite {
    fn mark_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool>;

    fn clear_needs_check(&mut self, path: &RepoPathBuf) -> Result<bool>;

    fn set_clock(&mut self, clock: Clock) -> Result<()>;

    fn flush(self, locker: &RepoLocker) -> Result<()>;
}

pub trait WatchmanTreeStateRead {
    fn list_needs_check(
        &mut self,
        matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    ) -> Result<(Vec<RepoPathBuf>, Vec<ParseError>)>;

    fn get_clock(&self) -> Result<Option<Clock>>;
}

#[derive(Clone)]
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
    fn list_needs_check(
        &mut self,
        matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    ) -> Result<(Vec<RepoPathBuf>, Vec<ParseError>)> {
        let mut needs_check = Vec::new();

        let parse_errs = walk_treestate(
            &mut self.treestate.lock(),
            matcher,
            StateFlags::NEED_CHECK,
            StateFlags::empty(),
            |path, _state| {
                needs_check.push(path);
                Ok(())
            },
        )?;

        Ok((needs_check, parse_errs))
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

#[cfg(test)]
mod tests {
    use pathmatcher::ExactMatcher;
    use types::RepoPath;

    use super::*;

    #[test]
    fn test_skip_ignored_files() -> Result<()> {
        // Show that we respect the matcher to skip treestate files we don't care about.

        let fs = FileStateV2 {
            mode: 0,
            size: 0,
            mtime: 0,
            state: StateFlags::NEED_CHECK,
            copied: None,
        };

        let dir = tempfile::tempdir()?;

        let mut ts = TreeState::new(dir.path(), false)?.0;
        ts.insert("include_me", &fs)?;
        ts.insert("ignore_me", &fs)?;

        let matcher = Arc::new(ExactMatcher::new(
            [RepoPath::from_str("include_me")?].iter(),
            false,
        ));

        let mut wm_ts = WatchmanTreeState {
            treestate: Arc::new(Mutex::new(ts)),
            root: "/dev/null".as_ref(),
        };

        let (needs_check, _) = wm_ts.list_needs_check(matcher)?;
        assert_eq!(
            needs_check,
            vec![RepoPathBuf::from_string("include_me".to_string())?],
        );

        Ok(())
    }
}
