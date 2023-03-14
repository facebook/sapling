/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use pathmatcher::Matcher;
use repolock::RepoLocker;
use treestate::dirstate;
use treestate::filestate::FileStateV2;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use treestate::ErrorKind;
use types::path::ParseError;
use types::RepoPathBuf;
use watchman_client::prelude::*;

use crate::util::walk_treestate;

pub fn mark_needs_check(ts: &mut TreeState, path: &RepoPathBuf) -> Result<bool> {
    let state = ts.get(path)?;
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
    ts.insert(path, &filestate)?;
    Ok(true)
}

pub fn clear_needs_check(ts: &mut TreeState, path: &RepoPathBuf) -> Result<bool> {
    let state = ts.get(path)?;
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

        if filestate.state.is_empty() {
            // No other flags means it was ignored/untracked, but now we don't
            // care about it (either it was deleted, or we aren't tracking
            // ignored files anymore).
            ts.remove(path)?;
        } else {
            ts.insert(path, &filestate)?;
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn set_clock(ts: &mut TreeState, clock: Clock) -> Result<()> {
    let clock_string = match clock {
        Clock::Spec(ClockSpec::StringClock(string)) => Ok(string),
        clock => Err(anyhow!(
            "Watchman implementation only handles opaque string type. Got the following clock instead: {:?}",
            clock
        )),
    }?;

    ts.update_metadata(&[("clock".to_string(), Some(clock_string))])?;

    Ok(())
}

pub fn maybe_flush_treestate(root: &Path, ts: &mut TreeState, locker: &RepoLocker) -> Result<()> {
    match dirstate::flush(root, ts, locker) {
        Ok(()) => Ok(()),
        // If the dirstate was changed before we flushed, that's ok. Let the other write win
        // since writes during status are just optimizations.
        Err(e) => match e.downcast_ref::<ErrorKind>() {
            Some(e) if *e == ErrorKind::TreestateOutOfDate => Ok(()),
            _ => Err(e),
        },
    }
}

pub fn list_needs_check(
    ts: &mut TreeState,
    matcher: Arc<dyn Matcher + Send + Sync + 'static>,
) -> Result<(Vec<RepoPathBuf>, Vec<ParseError>)> {
    let mut needs_check = Vec::new();

    let parse_errs = walk_treestate(
        ts,
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

pub fn get_clock(metadata: &BTreeMap<String, String>) -> Result<Option<Clock>> {
    Ok(metadata
        .get(&"clock".to_string())
        .map(|clock| Clock::Spec(ClockSpec::StringClock(clock.clone()))))
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

        let (needs_check, _) = list_needs_check(&mut ts, matcher)?;
        assert_eq!(
            needs_check,
            vec![RepoPathBuf::from_string("include_me".to_string())?],
        );

        Ok(())
    }
}
