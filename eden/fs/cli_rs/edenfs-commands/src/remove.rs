/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl remove
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use dialoguer::Confirm;
use edenfs_client::EdenFsClient;
use edenfs_client::EdenFsInstance;
use edenfs_error::EdenFsError;
use edenfs_utils::bytes_from_path;
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
    client: Option<Result<EdenFsClient>>,
}

impl RemoveContext {
    fn new(
        original_path: String,
        skip_prompt: bool,
        client: Result<EdenFsClient, EdenFsError>,
    ) -> RemoveContext {
        RemoveContext {
            original_path,
            canonical_path: PathBuf::new(),
            skip_prompt,
            client: match client {
                Ok(client) => Some(Ok(client)),
                Err(e) => {
                    warn!("Failed to initialize EdenFsClient: {e}");
                    Some(Err(anyhow!("{e}")))
                }
            },
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
        // TODO: stop process first
        match self.unmount(context).await {
            Ok(_) => {
                debug!("unmount path {} done", context);
                Ok(Some(State::InactiveEdenMount(InactiveEdenMount {})))
            }
            Err(e) => {
                error!("Failed to unmount {}: {e}", context);
                Ok(None)
            }
        }
    }

    async fn unmount(&self, context: &RemoveContext) -> Result<()> {
        match context.client {
            Some(ref client_res) => match client_res {
                Ok(client) => {
                    debug!("trying to unmount {}", context);
                    let encoded_path = bytes_from_path(context.canonical_path.clone());
                    match encoded_path {
                        Ok(path) => {
                            let umount_res = client.unmount(&path).await;
                            match umount_res {
                                Ok(_) => Ok(()),
                                Err(e) => Err(anyhow!("Failed to unmount {}: {e}", context)),
                            }
                        }
                        Err(e) => Err(anyhow!("Failed to encode path {}: {e}", context)),
                    }
                }

                Err(e) => Err(anyhow!("{e}")),
            },
            None => {
                panic!("Failed to unmount due to missing EdenFsClient!")
            }
        }
    }
}

#[derive(Debug)]
struct InactiveEdenMount {}
impl InactiveEdenMount {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        self.remove_client_config_dir(context)?;
        self.remove_client_config_entry(context)?;

        Ok(Some(State::CleanUp(CleanUp {})))
    }

    fn remove_client_config_dir(&self, context: &RemoveContext) -> Result<()> {
        let instance = EdenFsInstance::global();

        match fs::remove_dir_all(instance.client_dir_for_mount_point(&context.canonical_path)?) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!(
                "Failed to remove client config directory for {}: {e}",
                context
            )),
        }
    }

    fn remove_client_config_entry(&self, context: &RemoveContext) -> Result<()> {
        let instance = EdenFsInstance::global();

        match instance.remove_path_from_directory_map(&context.canonical_path) {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!(
                "Failed to remove {} from config json file: {e}",
                context
            )),
        }
    }
}

#[derive(Debug)]
struct CleanUp {}
impl CleanUp {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        if context.skip_prompt
            || Confirm::new()
                .with_prompt("CleanUp State is not implemented yet... proceed?")
                .interact()?
        {
            return Err(anyhow!("Rust remove(CleanUp) is not implemented!"));
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
    InactiveEdenMount(InactiveEdenMount),
    CleanUp(CleanUp),
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
                State::CleanUp(_) => "CleanUp",
                State::InactiveEdenMount(_) => "InactiveEdenMount",
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
            State::InactiveEdenMount(inner) => inner.next(context).await,
            State::CleanUp(inner) => inner.next(context).await,
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

        let instance = EdenFsInstance::global();
        let client = instance.connect(None).await;

        let mut context = RemoveContext::new(self.paths[0].clone(), self.skip_prompt, client);
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

    fn default_context(original_path: String) -> RemoveContext {
        RemoveContext {
            original_path,
            canonical_path: PathBuf::new(),
            skip_prompt: true,
            client: None,
        }
    }

    #[tokio::test]
    async fn test_sanity_check_pass() {
        let tmp_dir = prepare_directory();
        let path = format!("{}/test/nested/../nested", tmp_dir.path().to_str().unwrap());
        let mut context = default_context(path);
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
        let mut context = default_context(path);
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
        let mut file_context = default_context(file_path_buf.display().to_string());
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
        let mut dir_context = default_context(temp_dir.path().to_str().unwrap().to_string());
        state = State::start().run(&mut dir_context).await.unwrap().unwrap();
        assert!(
            matches!(state, State::Determination(_)),
            "Expected Determination state"
        );
        assert!(state.run(&mut dir_context).await.is_err());
    }
}
