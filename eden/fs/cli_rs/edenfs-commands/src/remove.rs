/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl remove
use std::fmt;
use std::fs;
#[cfg(unix)]
use std::fs::Permissions;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

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
use fail::fail_point;
use io::IO;
use termlogger::TermLogger;
use tracing::debug;
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
    io: Arc<Messenger>,
}

impl RemoveContext {
    fn new(
        original_path: String,
        canonical_path: PathBuf,
        preserve_mount_point: bool,
        io: Arc<Messenger>,
    ) -> RemoveContext {
        RemoveContext {
            original_path,
            canonical_path,
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

async fn remove_active_eden_mount(context: &RemoveContext) -> Result<()> {
    // TODO: stop process first
    context
        .io
        .info(format!("Unmounting repo at {} ...", context.original_path));

    let instance = EdenFsInstance::global();

    match instance.unmount(&context.canonical_path).await {
        Ok(_) => {
            context.io.done();
            remove_inactive_eden_mount(context).await
        }
        Err(e) => Err(anyhow!(
            "Failed to unmount mount point at {}: {}",
            context,
            e
        )),
    }
}

async fn remove_inactive_eden_mount(context: &RemoveContext) -> Result<()> {
    context.io.info(format!(
        "Unregistering repo {} from EdenFS configs...",
        context.original_path
    ));
    remove_client_config_dir(context)?;
    remove_client_config_entry(context)?;

    context.io.done();

    clean_up(context).await
}

fn remove_client_config_dir(context: &RemoveContext) -> Result<()> {
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

fn remove_client_config_entry(context: &RemoveContext) -> Result<()> {
    let instance = EdenFsInstance::global();

    instance
        .remove_path_from_directory_map(&context.canonical_path)
        .with_context(|| format!("Failed to remove {} from config json file", context))
}

async fn clean_up(context: &RemoveContext) -> Result<()> {
    if context.preserve_mount_point {
        context.io.warn(format!(
            "preserve_mount_point flag is set, not removing the mount point {}!",
            context.original_path
        ));
        Ok(())
    } else {
        context.io.info(format!(
            "Cleaning up the directory {} ...",
            context.original_path
        ));
        clean_mount_point(&context.canonical_path)
            .with_context(|| anyhow!("Failed to clean mount point {}", context))?;
        context.io.done();

        validate_removal_completion(context).await
    }
}

#[cfg(unix)]
fn clean_mount_point(path: &Path) -> Result<()> {
    let perms = Permissions::from_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("Failed to set permission 755 for path {}", path.display()))?;
    forcefully_remove_dir_all(path)
        .with_context(|| format!("Failed to remove mount point {}", path.display()))
}

#[cfg(windows)]
fn clean_mount_point(path: &Path) -> Result<()> {
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

#[derive(Debug, Copy, Clone)]
enum PathType {
    ActiveEdenMount,
    InactiveEdenMount,
    RegularFile,
    Unknown,
}

impl PathType {
    fn get_prompt(&self, paths: Vec<&str>) -> String {
        let prompt_str = match self {
            PathType::ActiveEdenMount | PathType::InactiveEdenMount => format!(
                "Warning: this operation will permanently delete the following EdenFS checkouts:\n\
         \n\
         {}\n\
         \n\
         Any uncommitted changes and shelves in this checkout will be lost forever.\n",
                paths.join("\n")
            ),

            PathType::RegularFile => format!(
                "Warning: this operation will permanently delete the following files:\n\
        \n\
        {}\n\
        \n\
        After deletion, they will be lost forever.\n",
                paths.join("\n")
            ),

            PathType::Unknown => format!(
                "Warning: the following paths are directories not managed by EdenFS:\n\
        \n\
        {}\n\
        \n\
                Any files in them will be lost forever. \n",
                paths.join("\n")
            ),
        };
        prompt_str.yellow().to_string()
    }

    async fn remove(&self, context: &RemoveContext) -> Result<()> {
        match self {
            PathType::ActiveEdenMount => remove_active_eden_mount(context).await,
            PathType::InactiveEdenMount => remove_inactive_eden_mount(context).await,
            PathType::RegularFile => {
                fs::remove_file(context.canonical_path.as_path()).map_err(Into::into)
            }
            PathType::Unknown => clean_up(context).await,
        }
    }
}

// Validate and canonicalize the given path into absolute path with the type of PathBuf.
// Then determine a type for this path.
//
// Returns a tuple of:
//   1. Canonicalized path
//   2. Type of path
async fn classify_path(path: &str) -> Result<(PathBuf, PathType)> {
    let path_buf = PathBuf::from(path);

    match path_buf.canonicalize() {
        Err(e) => Err(e.into()),
        Ok(canonicalized_path) => {
            let path = canonicalized_path.as_path();
            if path.is_file() {
                return Ok((canonicalized_path, PathType::RegularFile));
            }

            if !path.is_dir() {
                // This is rare, but when it happens we should warn it.
                warn!(
                    "path {} is not a file or directory, please make sure it exists and you have permission to it.",
                    path.display()
                );
                return Err(anyhow!("Not a file or directory"));
            }

            debug!("{} is determined as a directory", path.display());

            if is_active_eden_mount(path) {
                debug!(
                    "path {} is determined to be an active EdenFS mount",
                    path.display()
                );

                return Ok((canonicalized_path, PathType::ActiveEdenMount));
            }

            debug!("{} is not an active EdenFS mount", path.display());

            // Check if it's a directory managed under eden
            let mut path_copy = canonicalized_path.clone();
            loop {
                if path_copy.pop() {
                    if is_active_eden_mount(&path_copy) {
                        let err_msg = format!(
                            "{} is not the root of checkout {}, not removing",
                            path.display(),
                            path_copy.display()
                        );
                        return Err(anyhow!(err_msg));
                    } else {
                        continue;
                    }
                }
                break;
            }

            // Maybe it's a directory that is left after unmount
            // If so, unregister it and clean from there
            match path_in_eden_config(path).await {
                Ok(true) => {
                    return Ok((canonicalized_path, PathType::InactiveEdenMount));
                }
                Err(e) => {
                    return Err(e);
                }
                _ => (),
            }

            // It's a directory that is not listed inside config.json
            // We don't know how to handle it properly, so move to "Unknown" state
            // and try to handle from there with "the best efforts".
            Ok((canonicalized_path, PathType::Unknown))
        }
    }
}

#[cfg(unix)]
fn is_active_eden_mount(path: &Path) -> bool {
    // For Linux and Mac, an active Eden mount should have a dir named ".eden" under the
    // repo root and there should be a symlink named "root" which points to the repo root
    let unix_eden_dot_dir_path = path.join(".eden").join("root");

    match unix_eden_dot_dir_path.canonicalize() {
        Ok(resolved_path) => resolved_path == path,
        _ => false,
    }
}

#[cfg(windows)]
fn is_active_eden_mount(path: &Path) -> bool {
    // For Windows, an active EdenFS mount should have a dir named ".eden" under the
    // repo and there should be a file named "config" under the ".eden" dir
    let config_path = path.join(".eden").join("config");
    if !config_path.exists() {
        return false;
    }
    true
}

async fn validate_removal_completion(context: &RemoveContext) -> Result<()> {
    context
        .io
        .info("Checking eden mount list and file system to verify the removal...".to_string());
    // check eden list
    if path_in_eden_config(context.canonical_path.as_path()).await? {
        return Err(anyhow!("Repo {} is still mounted", context));
    }

    fail_point!("remove:validate", |_| {
        Err(anyhow!("failpoint: expected failure"))
    });

    // check directory clean up
    if !context.preserve_mount_point {
        match context.canonical_path.try_exists() {
            Ok(false) => {
                context.io.done();
                Ok(())
            }
            Ok(true) => Err(anyhow!("Directory left by repo {} is not removed", context)),
            Err(e) => Err(anyhow!(
                "Failed to check the status of path {}: {}",
                context,
                e
            )),
        }
    } else {
        Ok(())
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
                "Both '-y' and '-n' are provided. This is not supported.\nExiting."
            ));
        }

        let (canonicalized_path, path_type) = classify_path(&self.paths[0]).await?;

        let messenger = Arc::new(Messenger::new(
            IO::stdio(),
            self.skip_prompt,
            self.suppress_output,
            self.no,
        ));

        if !self.skip_prompt {
            let prompt = path_type.get_prompt(vec![&self.paths[0]]);

            if !messenger.prompt_user(prompt)? {
                return Err(anyhow!(
                    "User did not confirm the removal. Stopping. Nothing removed!"
                ));
            }
        }

        let context = RemoveContext::new(
            self.paths[0].clone(),
            canonicalized_path,
            self.preserve_mount_point,
            messenger.clone(),
        );

        path_type.remove(&context).await?;

        context
            .io
            .success(format!("\nSuccessfully removed {}", context.original_path));
        Ok(0)
    }
}

async fn path_in_eden_config(path: &Path) -> Result<bool> {
    let mut mounts = get_mounts(EdenFsInstance::global())
        .await
        .with_context(|| anyhow!("Failed to call eden list"))?;
    let entry_key = dunce::simplified(path);
    mounts.retain(|mount_path_key, _| dunce::simplified(mount_path_key) == entry_key);

    Ok(!mounts.is_empty())
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

    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Context;
    use tempfile::tempdir;
    use tempfile::TempDir;

    use super::*;

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
    async fn test_classify_path_regular_file() {
        let temp_dir = prepare_directory();
        let file_path_buf = temp_dir.path().join("temporary-file.txt");
        fs::write(file_path_buf.as_path(), "anything").unwrap_or_else(|err| {
            panic!(
                "cannot write to a file at {}: {}",
                file_path_buf.display(),
                err
            )
        });

        let result = classify_path(file_path_buf.to_str().unwrap()).await;
        assert!(
            result.is_ok(),
            "path of a regular file should be classified"
        );
        let (p, t) = result.unwrap();
        assert!(
            p == file_path_buf.canonicalize().unwrap(),
            "path of a regular file should be canonicalized"
        );
        assert!(
            matches!(t, PathType::RegularFile),
            "path of a regular file should be classified as RegFile"
        );
    }

    #[tokio::test]
    async fn test_classify_nonexistent_path() {
        let tmp_dir = prepare_directory();
        let path = format!("{}/test/no_file", tmp_dir.path().to_str().unwrap());
        let path_buf = PathBuf::from(path);
        let result = classify_path(path_buf.to_str().unwrap()).await;
        assert!(
            result.is_err(),
            "nonexistent path should not be canonicalized"
        );
    }
}
