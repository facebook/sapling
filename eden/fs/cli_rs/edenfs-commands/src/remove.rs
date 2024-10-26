/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl remove
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use dialoguer::Confirm;
use tracing::debug;
use tracing::error;
use tracing::warn;

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
    skip_prompt: bool,
}

impl RemoveContext {
    fn new(original_path: String, skip_prompt: bool) -> RemoveContext {
        RemoveContext {
            original_path,
            canonical_path: PathBuf::new(),
            skip_prompt,
        }
    }
}

impl fmt::Display for RemoveContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.canonical_path.display())
    }
}

#[derive(Debug)]
struct SanityCheck {}
impl SanityCheck {
    /// This is the first step of the remove process. It will verify that the path is valid and exists.
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
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
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        let path = context.canonical_path.as_path();

        if path.is_file() {
            debug!("path {} determined to be a regular file", context);
            return Ok(Some(State::RegFile(RegFile {})));
        }

        if !path.is_dir() {
            return Err(anyhow!(format!("{} is not a file or a directory", context)));
        }

        debug!("{} is determined as a directory", context);

        if self.is_active_eden_mount(context) {
            debug!("path {} is determined to be an active eden mount", context);

            return Ok(Some(State::ActiveEdenMount(ActiveEdenMount {})));
        }

        error!("Determination State for directory is not implemented!");
        Err(anyhow!("Rust remove(Determination) is not implemented!"))
    }

    #[cfg(unix)]
    fn is_active_eden_mount(&self, context: &RemoveContext) -> bool {
        // For Linux and Mac, an active Eden mount should have a dir named ".eden" under the
        // repo root and there should be a symlink named "root" which points to the repo root
        let unix_eden_dot_dir_path = context.canonical_path.join(".eden").join("root");

        match unix_eden_dot_dir_path.canonicalize() {
            Ok(resolved_path) => resolved_path == context.canonical_path,
            Err(_) => {
                warn!("{} is not an active eden mount", context);
                false
            }
        }
    }

    #[cfg(windows)]
    fn is_active_eden_mount(&self, context: &RemoveContext) -> bool {
        warn!("is_active_eden_mount() unimplemented for Windows");
        false
    }
}

#[derive(Debug)]
struct RegFile {}
impl RegFile {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        if context.skip_prompt
            || Confirm::new()
                .with_prompt("RegFile State is not implemented yet... proceed?")
                .interact()?
        {
            return Err(anyhow!("Rust remove(RegFile) is not implemented!"));
        }
        Ok(None)
    }
}

#[derive(Debug)]
struct ActiveEdenMount {}
impl ActiveEdenMount {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        if context.skip_prompt
            || Confirm::new()
                .with_prompt("ActiveEdenMount State is not implemented yet... proceed?")
                .interact()?
        {
            return Err(anyhow!("Rust remove(ActiveEdenMount) is not implemented!"));
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
    ActiveEdenMount(ActiveEdenMount),
    // InactiveEdenMount,
    // CleanUp,
    RegFile(RegFile),
    // Unknown,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                State::SanityCheck(_) => "SanityCheck",
                State::Determination(_) => "Determination",
                State::RegFile(_) => "RegFile",
                State::ActiveEdenMount(_) => "ActiveEdenMount",
            }
        )
    }
}

impl State {
    fn start() -> State {
        State::SanityCheck(SanityCheck {})
    }

    /// Runs the actions defined for this state
    /// There are three cases for the return value:
    /// 1. Ok(Some(State)) - we succeed in moving to the next state
    /// 2. Ok(None) - we are in a terminal state and the removal is successful
    /// 3. Err - the removal failed
    async fn run(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        debug!("State {} running...", self);
        match self {
            State::SanityCheck(inner) => inner.next(context).await,
            State::Determination(inner) => inner.next(context).await,
            State::RegFile(inner) => inner.next(context).await,
            State::ActiveEdenMount(inner) => inner.next(context).await,
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

        let mut context = RemoveContext::new(self.paths[0].clone(), self.skip_prompt);
        let mut state = Some(State::start());

        while state.is_some() {
            match state.unwrap().run(&mut context).await {
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

    #[tokio::test]
    async fn test_sanity_check_pass() {
        let tmp_dir = prepare_directory();
        let path = format!("{}/test/nested/../nested", tmp_dir.path().to_str().unwrap());
        let mut context = RemoveContext::new(path, true);
        let state = State::start().run(&mut context).await.unwrap().unwrap();

        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        assert!(context.canonical_path.ends_with("test/nested"));
    }

    #[tokio::test]
    async fn test_sanity_check_fail() {
        let tmp_dir = prepare_directory();
        let path = format!(
            "{}/test/nested/../../nested/inner",
            tmp_dir.path().to_str().unwrap()
        );
        let mut context = RemoveContext::new(path, true);
        let state: std::result::Result<Option<State>, anyhow::Error> =
            State::start().run(&mut context).await;
        assert!(state.is_err());
        assert!(
            state
                .unwrap_err()
                .to_string()
                .contains(PATH_NOT_FOUND_ERROR_MSG)
        );
    }

    #[tokio::test]
    async fn test_determine_regular_file() {
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
        let mut file_context = RemoveContext::new(file_path_buf.display().to_string(), true);
        let mut state = State::start()
            .run(&mut file_context)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        state = state.run(&mut file_context).await.unwrap().unwrap();
        assert!(matches!(state, State::RegFile(_)), "Expected RegFile state");

        // When context includes a path to a directory
        let mut dir_context =
            RemoveContext::new(temp_dir.path().to_str().unwrap().to_string(), true);
        state = State::start().run(&mut dir_context).await.unwrap().unwrap();
        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        assert!(state.run(&mut dir_context).await.is_err());
    }
}
