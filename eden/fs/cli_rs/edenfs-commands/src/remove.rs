/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl remove

use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use dialoguer::Confirm;
use tracing::debug;
use tracing::error;

use crate::ExitCode;
use crate::Subcommand;

#[derive(Parser, Debug)]
#[clap(name = "remove", about = "Remove an EdenFS checkout")]
pub struct RemoveCmd {
    #[clap(
        multiple_values = true,
        help = "The EdenFS checkout(s) to remove.",
        value_name = "PATH"
    )]
    paths: Vec<String>,

    #[clap(
            short = 'y',
            long = "yes",
            visible_aliases = &["--no-prompt"],
            help = "Do not prompt for confirmation before removing the checkouts."
        )]
    skip_prompt: bool,

    #[clap(long, hide = true)]
    preserve_mount_point: bool,
}

struct RemoveContext {
    original_path: String,
    canonical_path: PathBuf,
}

impl RemoveContext {
    fn new(original_path: String) -> RemoveContext {
        RemoveContext {
            original_path,
            canonical_path: PathBuf::new(),
        }
    }
}

#[derive(Debug)]
struct SanityCheck {}
impl SanityCheck {
    /// This is the first step of the remove process. It will verify that the path is valid and exists.
    fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        match Path::new(&context.original_path).canonicalize() {
            // cannonicalize() will check if the path exists for us so this is all we need
            Ok(path) => {
                context.canonical_path = path;
                Ok(Some(State::Determination(Determination {})))
            }
            Err(e) => Err(anyhow!(
                "Error canonicalizing path {}: {}",
                context.original_path,
                e
            )),
        }
    }
}

#[derive(Debug)]
struct Determination {}
impl Determination {
    fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        let path = context.canonical_path.as_path();

        if path.is_file() {
            debug!("path {} determined to be a regular file", path.display());
            return Ok(Some(State::RegFile(RegFile {})));
        }

        error!("Determination State for directory is not implemented!");
        Err(anyhow!("Rust remove(Determination) is not implemented!"))
    }
}

#[derive(Debug)]
struct RegFile {}
impl RegFile {
    fn next(&self, _context: &mut RemoveContext) -> Result<Option<State>> {
        if Confirm::new()
            .with_prompt("RegFile State is not implemented yet... proceed?")
            .interact()?
        {
            return Err(anyhow!("Rust remove(RegFile) is not implemented!"));
        }
        Ok(None)
    }
}

#[derive(Debug)]
enum State {
    // function states (no real action performed)
    SanityCheck(SanityCheck),
    Determination(Determination),
    // Validation,

    // // removal states (harmful operations)
    // ActiveEdenMount,
    // InactiveEdenMount,
    // CleanUp,
    RegFile(RegFile),
    // Unknown,
}

impl State {
    fn start() -> State {
        State::SanityCheck(SanityCheck {})
    }

    fn name(&self) -> &'static str {
        match self {
            State::SanityCheck(_) => "SanityCheck",
            State::Determination(_) => "Determination",
            State::RegFile(_) => "RegFile",
        }
    }

    /// Runs the actions defined for this state
    /// There are three cases for the return value:
    /// 1. Ok(Some(State)) - we succeed in moving to the next state
    /// 2. Ok(None) - we are in a terminal state and the removal is successful
    /// 3. Err - the removal failed
    fn run(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        debug!("State {} running...", self.name());
        match self {
            State::SanityCheck(inner) => inner.next(context),
            State::Determination(inner) => inner.next(context),
            State::RegFile(inner) => inner.next(context),
        }
    }
}

#[async_trait]
impl Subcommand for RemoveCmd {
    async fn run(&self) -> Result<ExitCode> {
        // TODO: remove this check eventually because we should be able to remove multiple paths
        assert!(
            self.paths.len() == 1,
            "Currently supporting only one path given per run"
        );

        let mut context = RemoveContext::new(self.paths[0].clone());
        let mut state = Some(State::start());

        while state.is_some() {
            match state.unwrap().run(&mut context) {
                Ok(next_state) => state = next_state,
                Err(e) => {
                    // TODO: handling error processing like logging, etc
                    return Err(e);
                }
            }
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Context;
    use tempfile::tempdir;
    use tempfile::TempDir;

    use super::*;

    #[cfg(unix)]
    const PATH_NOT_FOUND_ERROR_MSG: &str = "No such file or directory";

    #[cfg(windows)]
    const PATH_NOT_FOUND_ERROR_MSG: &str = "The system cannot find the path specified";

    /// This helper function creates a directory structure that looks like this:
    /// "some_tmp_dir/test/nested/inner"
    /// then it returns the path to the "some_tmp_dir" directory
    fn prepare_directory() -> TempDir {
        let temp_dir = tempdir().context("couldn't create temp dir").unwrap();
        let path = temp_dir.path().join("test").join("nested").join("inner");
        let prefix = path.parent().unwrap();
        println!("creating dirs: {:?}", prefix.to_str().unwrap());
        std::fs::create_dir_all(prefix).unwrap();
        temp_dir
    }

    #[test]
    fn test_sanity_check_pass() {
        let tmp_dir = prepare_directory();
        let path = format!("{}/test/nested/../nested", tmp_dir.path().to_str().unwrap());
        let mut context = RemoveContext::new(path);
        let state = State::start().run(&mut context).unwrap().unwrap();

        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        assert!(context.canonical_path.ends_with("test/nested"));
    }

    #[test]
    fn test_sanity_check_fail() {
        let tmp_dir = prepare_directory();
        let path = format!(
            "{}/test/nested/../../nested/inner",
            tmp_dir.path().to_str().unwrap()
        );
        let mut context = RemoveContext::new(path);
        let state: std::result::Result<Option<State>, anyhow::Error> =
            State::start().run(&mut context);
        assert!(state.is_err());
        assert!(
            state
                .unwrap_err()
                .to_string()
                .contains(PATH_NOT_FOUND_ERROR_MSG)
        );
    }

    #[test]
    fn test_determine_regular_file() {
        let temp_dir = prepare_directory();
        let file_path_buf = temp_dir.path().join("temporary-file.txt");
        fs::write(file_path_buf.as_path(), "anything").unwrap_or_else(|err| {
            panic!(
                "cannot write to a file at {}: {}",
                file_path_buf.display(),
                err
            )
        });

        // When context includes a path to a regular file
        let mut file_context = RemoveContext::new(file_path_buf.display().to_string());
        let mut state = State::start().run(&mut file_context).unwrap().unwrap();
        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        state = state.run(&mut file_context).unwrap().unwrap();
        assert!(matches!(state, State::RegFile(_)), "Expected RegFile state");

        // When context includes a path to a directory
        let mut dir_context = RemoveContext::new(temp_dir.path().to_str().unwrap().to_string());
        state = State::start().run(&mut dir_context).unwrap().unwrap();
        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        assert!(state.run(&mut dir_context).is_err());
    }
}
