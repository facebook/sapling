/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl remove
use std::fmt;
use std::fs;
use std::fs::Permissions;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use crossterm::style::Stylize;
use dialoguer::Confirm;
use edenfs_client::checkout::get_mounts;
use edenfs_client::fsutil::forcefully_remove_dir_all;
use edenfs_client::EdenFsInstance;
use edenfs_utils::bytes_from_path;
use io::IO;
use termlogger::TermLogger;
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

    // Do not print to stdout. This is independent with '--no-prompt'
    #[clap(short = 'q', long = "quiet", hide = true)]
    suppress_output: bool,

    // Answer no for any prompt.
    // This is only used in testing the path when a user does not confirm upon the prompt
    // I have to this because dialoguer::Confirm does not accept input from non-terminal
    // https://github.com/console-rs/dialoguer/issues/170
    //
    // When provided with "-y": undefined!
    #[clap(short = 'n', long = "answer-no", hide = true)]
    no: bool,

    #[clap(long, hide = true)]
    preserve_mount_point: bool,
}

struct RemoveContext {
    original_path: String,
    canonical_path: PathBuf,
    preserve_mount_point: bool,
    io: Messenger,
}

impl RemoveContext {
    fn new(original_path: String, preserve_mount_point: bool, io: Messenger) -> RemoveContext {
        RemoveContext {
            original_path,
            canonical_path: PathBuf::new(),
            preserve_mount_point,
            io,
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
        let path = Path::new(&context.original_path)
            .canonicalize()
            .with_context(|| format!("Error canonicalizing path {}", context.original_path))?;
        context.canonical_path = path;

        if context.io.prompt_user(construct_start_prompt(context))? {
            return Ok(Some(State::Determination(Determination {})));
        }

        Err(anyhow!(
            "User did not confirm the removal. Stopping. Nothing removed!"
        ))
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
        // For Windows, an active Eden mount should have a dir named ".eden" under the
        // repo and there should be a file named "config" under the ".eden" dir
        let config_path = context.canonical_path.join(".eden").join("config");
        if !config_path.exists() {
            warn!("{} is not an active eden mount", context);
            return false;
        }
        true
    }
}

#[derive(Debug)]
struct RegFile {}
impl RegFile {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        if context
            .io
            .prompt_user("{} is a file, do you still want to remove it?".to_string())?
        {
            fs::remove_file(context.canonical_path.as_path())
                .with_context(|| format!("Failed to remove mount point {}", context))?;
            return Ok(None);
        }

        Err(anyhow!(
            "User did not confirm the removal. Stopping. Nothing removed!"
        ))
    }
}

#[derive(Debug)]
struct ActiveEdenMount {}
impl ActiveEdenMount {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        // TODO: stop process first

        context
            .io
            .info(format!("Unmounting repo at {} ...", context.original_path));

        match self.unmount(context).await {
            Ok(_) => {
                context.io.done();
                Ok(Some(State::InactiveEdenMount(InactiveEdenMount {})))
            }
            Err(e) => Err(anyhow!(
                "Failed to unmount mount point at {}: {}",
                context,
                e
            )),
        }
    }

    async fn unmount(&self, context: &RemoveContext) -> Result<()> {
        debug!("trying to unmount {}", context);
        let encoded_path = bytes_from_path(context.canonical_path.clone())
            .with_context(|| format!("Failed to encode path {}", context))?;
        let instance = EdenFsInstance::global();
        let client = instance.connect(None).await?;
        client
            .unmount(&encoded_path)
            .await
            .with_context(|| format!("Failed to unmount {}", context))
    }
}

#[derive(Debug)]
struct InactiveEdenMount {}
impl InactiveEdenMount {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        context.io.info(format!(
            "Unregistering repo {} from Eden configs...",
            context.original_path
        ));
        self.remove_client_config_dir(context)?;
        self.remove_client_config_entry(context)?;

        context.io.done();

        Ok(Some(State::CleanUp(CleanUp {})))
    }

    fn remove_client_config_dir(&self, context: &RemoveContext) -> Result<()> {
        let instance = EdenFsInstance::global();

        match fs::remove_dir_all(instance.client_dir_for_mount_point(&context.canonical_path)?) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!(
                "Failed to remove client config directory for {}: {}",
                context,
                e
            )),
        }
    }

    fn remove_client_config_entry(&self, context: &RemoveContext) -> Result<()> {
        let instance = EdenFsInstance::global();

        instance
            .remove_path_from_directory_map(&context.canonical_path)
            .with_context(|| format!("Failed to remove {} from config json file", context))
    }
}

#[derive(Debug)]
struct CleanUp {}
impl CleanUp {
    async fn next(&self, context: &mut RemoveContext) -> Result<Option<State>> {
        if context.preserve_mount_point {
            context.io.warn(format!(
                "preserve_mount_point flag is set, not removing the mount point {}!",
                context.original_path
            ));
            Ok(None)
        } else {
            context.io.info(format!(
                "Cleaning up the directory left by repo {} ...",
                context.original_path
            ));
            self.clean_mount_point(&context.canonical_path)
                .await
                .with_context(|| anyhow!("Failed to clean mount point {}", context))?;
            context.io.done();

            Ok(Some(State::Validation))
        }
    }

    #[cfg(unix)]
    async fn clean_mount_point(&self, path: &Path) -> Result<()> {
        let perms = Permissions::from_mode(0o755);
        fs::set_permissions(path, perms)
            .with_context(|| format!("Failed to set permission 755 for path {}", path.display()))?;
        forcefully_remove_dir_all(path)
            .with_context(|| format!("Failed to remove mount point {}", path.display()))
    }

    #[cfg(windows)]
    async fn clean_mount_point(&self, path: &Path) -> Result<()> {
        // forcefully_remove_dir_all() is simply a wrapper of remove_dir_all() which handles the retry logic.
        //
        // There is a chance that remove_dir_all() can hit the error:
        // """
        // Failed to remove mount point \\?\C:\open\repo-for-safe-remove: The provider that supports,
        // file system virtualization is temporarily unavailable. (os error 369)
        // """
        //
        // Hopefully, retrying the command will fix the issue since it's temporary.
        // But if we keep seeing this error even after retrying, we should consider implementing
        // something similar to Remove-Item(rm) cmdlet from PowerShell.
        //
        // Note: It's known that "rm -f -r" should be able to remove the repo but we should not rely
        // on it from the code.
        forcefully_remove_dir_all(path)
            .with_context(|| anyhow!("Failed to remove repo directory {}", path.display()))
    }
}

async fn validate_state_run(context: &mut RemoveContext) -> Result<Option<State>> {
    context
        .io
        .info("Checking eden mount list and file system to verify the removal...".to_string());
    // check eden list
    let mut mounts = get_mounts(EdenFsInstance::global())
        .await
        .with_context(|| anyhow!("Failed to call eden list"))?;
    let entry_key = dunce::simplified(context.canonical_path.as_path());
    mounts.retain(|mount_path_key, _| dunce::simplified(mount_path_key) == entry_key);
    if !mounts.is_empty() {
        return Err(anyhow!("Repo {} is still mounted", context));
    }

    // check directory clean up
    if !context.preserve_mount_point {
        match context.canonical_path.try_exists() {
            Ok(false) => {
                context.io.done();
                Ok(None)
            }
            Ok(true) => Err(anyhow!("Directory left by repo {} is not removed", context)),
            Err(e) => Err(anyhow!(
                "Failed to check the status of path {}: {}",
                context,
                e
            )),
        }
    } else {
        Ok(None)
    }
}

#[derive(Debug)]
enum State {
    // function states (no real action performed)
    SanityCheck(SanityCheck),
    Determination(Determination),
    Validation,

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
                State::Validation => "Validation",
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
            State::Validation => validate_state_run(context).await,
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

        if self.skip_prompt && self.no {
            return Err(anyhow!(
                "Both '-y' and '-n' are provided. This is not supported.\nExisting."
            ));
        }

        let messenger =
            Messenger::new(IO::stdio(), self.skip_prompt, self.suppress_output, self.no);

        let mut context =
            RemoveContext::new(self.paths[0].clone(), self.preserve_mount_point, messenger);
        let mut state = Some(State::start());

        while state.is_some() {
            match state.unwrap().run(&mut context).await {
                Ok(next_state) => state = next_state,
                Err(e) => {
                    return Err(e);
                }
            }
        }

        context
            .io
            .success(format!("\nSuccessfully removed {}", context.original_path));
        Ok(0)
    }
}

// Object responsible to print messages to stdout or generate prompt
// for the user and receive response
struct Messenger {
    logger: TermLogger,
    skip_prompt: bool,
    answer_no: bool,
}

impl Messenger {
    fn new(io: IO, skip_prompt: bool, suppress_output: bool, answer_no: bool) -> Messenger {
        Messenger {
            logger: TermLogger::new(&io).with_quiet(suppress_output),
            skip_prompt,
            answer_no,
        }
    }

    fn info(&self, msg: String) {
        self.logger.info(msg);
    }

    fn warn(&self, msg: String) {
        self.logger.warn(msg.yellow().to_string());
    }

    fn error(&self, msg: String) {
        self.logger.warn(msg.red().to_string());
    }

    fn success(&self, msg: String) {
        self.logger.info(msg.green().to_string());
    }

    fn done(&self) {
        self.success("✓".to_string());
    }

    fn prompt_user(&self, prompt: String) -> Result<bool> {
        if self.answer_no {
            return Ok(false);
        }

        if !self.skip_prompt {
            self.logger.info(prompt);
            let res = Confirm::new().with_prompt("Proceed?").interact()?;
            return Ok(res);
        }
        Ok(true)
    }
}

fn construct_start_prompt(context: &RemoveContext) -> String {
    format!(
        "Warning: this operation will permanently delete the following checkouts:\n\
         \n\
         {}\n\
         \n\
         Any uncommitted changes and shelves in this checkout will be lost forever.\n",
        dunce::simplified(&context.canonical_path).to_string_lossy(),
    )
    .yellow()
    .to_string()
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
        let messenger = Messenger::new(IO::null(), true, true, false);
        RemoveContext {
            original_path,
            canonical_path: PathBuf::new(),
            preserve_mount_point: false,
            io: messenger,
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
                .root_cause()
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
