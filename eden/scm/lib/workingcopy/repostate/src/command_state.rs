/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::MergeState;

#[derive(Debug, Clone)]
pub struct State {
    // Sapling command name associated with this state.
    command: &'static str,

    description: &'static str,

    // If working copy .hg/<state_file> exists, we are in this state.
    // The file may be an empty touch file, or contain actual state.
    state_file: &'static str,

    // Operations that are allowed to proceed when in this state.
    allows: &'static [Operation],

    proceed: &'static str,
    abort: &'static str,
    abort_lossy: bool,
}

impl State {
    fn is_active(&self, dot_path: &Path) -> Result<bool> {
        Ok(util::file::exists(dot_path.join(self.state_file))?.is_some())
    }

    fn allow(&self, op: Operation) -> bool {
        self.allows.contains(&op)
    }
}

#[derive(Debug)]
pub struct Conflict(State);

impl Conflict {
    pub fn description(&self) -> &'static str {
        self.0.description
    }

    pub fn hint(&self) -> String {
        use std::fmt::Write;

        let mut s = String::new();
        let _ = write!(
            s,
            "use '{} {}' to continue or\n",
            identity::cli_name(),
            self.0.proceed
        );

        // This is indented assuming a "(" will preceed the hint when output.
        let _ = write!(
            s,
            "     '{} {}' to abort",
            identity::cli_name(),
            self.0.abort
        );
        if self.0.abort_lossy {
            let _ = write!(s, " - WARNING: will destroy uncommitted changes");
        }

        s
    }
}

impl Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())?;
        write!(f, "\n({})", self.hint())?;

        Ok(())
    }
}

impl std::error::Error for Conflict {}

// Order matters since we report the first matching/problematic state to the user.
// So, put more specific/exclusive states first.
static STATES: &[State] = &[
    State {
        // Interrupted "graft" due to conflicts.
        command: "graft",
        description: "graft in progress",
        state_file: "graftstate",
        allows: &[],
        proceed: "graft --continue",
        abort: "graft --abort",
        abort_lossy: false,
    },
    State {
        // Interrupted "go --merge" due to conflicts.
        command: "goto",
        description: "goto --merge in progress",
        state_file: "updatemergestate",
        allows: &[Operation::Commit],
        proceed: "goto --continue",
        abort: "goto --clean",
        abort_lossy: true,
    },
    State {
        // Interrupted "unshelve" due to conflicts.
        command: "unshelve",
        description: "unshelve in progress",
        state_file: "shelvedstate",
        allows: &[],
        proceed: "unshelve --continue",
        abort: "unshelve --abort",
        abort_lossy: false,
    },
    State {
        // Interrupted "rebase" due to conflicts.
        command: "rebase",
        description: "rebase in progress",
        state_file: "rebasestate",
        allows: &[],
        proceed: "rebase --continue",
        abort: "rebase --abort",
        abort_lossy: false,
    },
    State {
        // Interrupted "histedit" due to conflicts.
        command: "histedit",
        description: "histedit in progress",
        state_file: "histedit-state",
        // By design, "histedit" allows committing mid-operation. Commit will
        // still be rejected if there are unresolved conflicts.
        allows: &[Operation::Commit],
        proceed: "histedit --continue",
        abort: "histedit --abort",
        abort_lossy: false,
    },
    State {
        // Interrupted "goto" due to unexpected failure.
        command: "goto",
        description: "interrupted goto",
        state_file: "updatestate",
        allows: &[],
        proceed: "goto --continue",
        abort: "goto --clean",
        abort_lossy: true,
    },
];

static UNRESOLVED_CONFLICTS: State = State {
    // Interrupted "merge" due to conflicts.
    //
    // This can happen either from an actual "merge", or if an above state file
    // is deleted for whatever reason without cleaning up the merge state. The
    // goal with calling it out here is basically to tell the user to run "sl
    // goto --clean" to get out of this state.
    command: "merge",
    description: "unresolved merge state",
    state_file: "merge/state2",
    allows: &[Operation::GotoClean],
    proceed: "resolve",
    abort: "goto --clean",
    abort_lossy: true,
};

#[derive(Debug, Eq, PartialEq, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    GotoClean,
    Commit,
    Other,
}

pub fn try_operation(dot_path: &Path, op: Operation) -> Result<()> {
    // This originally took a &repolock::LockedPath, but that required taking
    // the wlock "randomly" from places in Python such as the "undo" extension
    // (which caused lock ordering and deadlock issues). This "should" require
    // the wlock, but at least for now that caused more problems than it solved.

    for s in STATES.iter() {
        if !s.is_active(dot_path)? {
            continue;
        }

        if !s.allow(op) {
            return Err(Conflict(s.clone()).into());
        }
    }

    if UNRESOLVED_CONFLICTS.is_active(dot_path)? && !UNRESOLVED_CONFLICTS.allow(op) {
        if MergeState::read(&dot_path.join(UNRESOLVED_CONFLICTS.state_file))?
            .map_or(false, |ms| ms.is_unresolved())
        {
            return Err(Conflict(UNRESOLVED_CONFLICTS.clone()).into());
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_advice() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        let config = BTreeMap::<String, String>::new();
        let locker = repolock::RepoLocker::new(&config, tmp.path().to_owned())?;
        let locked_path = locker.lock_working_copy(tmp.path().to_owned())?;

        assert!(try_operation(&locked_path, Operation::Other).is_ok());

        std::fs::File::create(locked_path.join("updatemergestate"))?;

        let err = try_operation(&locked_path, Operation::GotoClean).unwrap_err();
        let err: Conflict = err.downcast().unwrap();
        assert_eq!(
            format!("{err}"),
            "goto --merge in progress
(use 'sl goto --continue' to continue or
     'sl goto --clean' to abort - WARNING: will destroy uncommitted changes)"
        );

        Ok(())
    }
}
