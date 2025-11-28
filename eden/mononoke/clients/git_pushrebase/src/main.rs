// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

//! Git Pushrebase
//!
//! This is a binary that will replace `git push` calls for Git repos that are
//! synced to a Mononoke large repo.
//!
//! When the source of truth is still in the Git repo, this binary will
//! act as a wrapper for `git push`, supporting only a subset of arguments
//! that will also be supported after the source of truth is changed.

mod large_repo_push;
pub mod tests;
mod utils;

use std::fs::OpenOptions;
use std::io::Write;
use std::process::exit;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use clap::Parser;
use cmdlib_logging::LoggingArgs;
use fbinit::FacebookInit;
use serde_json::Value;
use tracing::debug;
use tracing::info;
use tracing::warn;
use utils::run_scsc_command;

use crate::large_repo_push::push_to_large_repo;
use crate::utils::build_git_command;
use crate::utils::run_git_command;

const COMMIT_CLOUD_REF: &str = "refs/commitcloud/upload";

#[derive(Debug, Parser)]
#[clap(about = "git push replacement for Git repos synced to a Mononoke large repo")]
pub struct GitPushrebaseArgs {
    #[clap(help = "Remote repository to push to")]
    pub remote: Option<String>,
    #[clap(help = "References (e.g. branch) to be pushed")]
    pub refspec: Option<String>,

    /// Max number of poll attempts, e.g. if the commits pushed to the large
    /// repo were backsynced or the master ref was updated in the remote repo.
    #[clap(long, default_value_t = 30)]
    pub max_polls: u64,
    /// Interval to wait between polls (in ms), e.g. commit was backsynced or
    /// master ref was updated in remote repo.
    #[clap(long, default_value_t = 2000)]
    pub poll_sleep_duration_ms: u64,

    #[clap(long, default_value = "master")]
    pub large_repo_pushrebase_bookmark: String,

    #[clap(flatten, next_help_heading = "LOGGING OPTIONS")]
    logging_args: LoggingArgs,
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let args = GitPushrebaseArgs::parse();

    _ = args.logging_args.setup_logging(fb)?;

    if let Some((git_repo_name, large_repo_name)) = data_for_large_repo_push(&args).await {
        println!(
            "The source of truth of {git_repo_name} is in {large_repo_name}. Pushes will be redirected to {large_repo_name} and backsynced to {git_repo_name}"
        );
        let result = push_to_large_repo(
            &git_repo_name,
            args.remote,
            args.refspec,
            &args.large_repo_pushrebase_bookmark,
            &large_repo_name,
        )
        .await;

        if let Err(err) = &result {
            if let Err(err) = log_to_git_trace2(&git_repo_name, &large_repo_name, err) {
                warn!("Failed to log to Git Trace2 file: {err:?}");
            }
            println!(
                "Failed to push to {large_repo_name}. Please refer the following wiki for help: https://fburl.com/whatsapp_git_pushrebase\n\n"
            );
        }

        return result;
    };

    debug!("Repo's source of truth is still in Git. Proceeding with standard git push.");

    // Run `git push` with provided args
    let mut command = build_git_command()?;
    command.arg("push");

    if let Some(remote) = &args.remote {
        command.arg(remote);
    }
    if let Some(refspec) = &args.refspec {
        command.arg(refspec);
    }

    let mut child = command
        .spawn()
        .context("Failed to spawn git push command")?;

    let exit_status = child.wait()?;

    exit(exit_status.code().unwrap_or(1))
}

/// Checks if the source of truth for this git repo is in a large repo, in which
/// case it returns the its name along with the git repo's name in Mononoke.
///
/// NOTE: this function should NOT crash explicitly because of unmet assumptions,
/// because it's called before the `git push` flow and it shouldn't impact any
/// repo that doesn't have their SoT in a large repo.
async fn data_for_large_repo_push(args: &GitPushrebaseArgs) -> Option<(String, String)> {
    if let Some(refspec) = &args.refspec {
        // Commit cloud uploads should skip large repo push and use vanilla git
        // push.
        if refspec.contains(COMMIT_CLOUD_REF) {
            debug!("Refspec contains {COMMIT_CLOUD_REF}. Skipping large repo push.");
            return None;
        }
    }
    let mb_git_repo_name = get_git_repo_name(&args.remote).unwrap_or_else(|err| {
        info!("Failed to get git repo name: {err}");
        None
    });

    if let Some(git_repo_name) = mb_git_repo_name {
        debug!("Querying source of truth for git repo: {git_repo_name}");
        // If this git repo is synced to a large repo and its source of truth is there,
        // get its name so that the commit can be synced and pushed there.
        let mb_large_repo_name = get_large_repo_name(&git_repo_name)
            .await
            .unwrap_or_else(|err| {
                info!("Failed to get large repo name: {err}");
                None
            });

        if let Some(large_repo_name) = mb_large_repo_name {
            debug!("Source of truth of git repo {git_repo_name} is {large_repo_name:?}");
            return Some((git_repo_name, large_repo_name));
        }

        debug!("Source of truth of git repo {git_repo_name} is still itself");
    }
    None
}

/// Get the name of the git repo where this command is running. The name of the
/// repo will be returned without the `.git` suffix, so it matches its name
/// in Mononoke.
fn get_git_repo_name(mb_remote: &Option<String>) -> Result<Option<String>> {
    let remote = mb_remote.clone().unwrap_or("origin".to_string());
    let remote_out = run_git_command(["remote", "-v"])?;

    get_git_repo_name_impl(&remote, remote_out)
}

fn get_git_repo_name_impl(remote: &str, git_remote_output: String) -> Result<Option<String>> {
    let remote_line = git_remote_output
        .split('\n')
        .filter(|line| line.contains("/rw") || line.contains("/ro"))
        .find(|line| line.contains(remote))
        .ok_or_else(|| anyhow!("Remote {remote} remote not found"))?;

    let mb_repo_name = remote_line
        .split("git/rw/")
        .nth(1)
        .and_then(|s| s.split_whitespace().next());

    let repo_name = if let Some(repo_name) = mb_repo_name {
        repo_name
    } else {
        remote_line
            .split("git/ro/")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .ok_or_else(|| anyhow!("Couldn't get reponame from remote: {remote_line}"))?
    };

    // Remove the `.git` suffix, if any
    Ok(repo_name.split(".git").next().map(String::from))
}

/// If this git repo is synced to a large repo and its source of truth is there,
/// use SCS client to get its name.
async fn get_large_repo_name(git_repo_name: &str) -> Result<Option<String>> {
    let repo_info_stdout = run_scsc_command(["repo-info", "--repo", git_repo_name, "--json"])?;
    let repo_info_json: Value =
        serde_json::from_str(&repo_info_stdout).context("Failed to parse scsc repo-info output")?;

    let mb_repo_name = repo_info_json["push_redirected_to"].as_str();

    Ok(mb_repo_name.map(String::from))
}

// mod test {}

fn log_to_git_trace2(
    git_repo_name: &str,
    large_repo_name: &str,
    error: &anyhow::Error,
) -> Result<()> {
    match std::env::var_os("GIT_TRACE2") {
        Some(git_trace_file_path) => {
            // Write error and some useful information to the
            // Git Trace2 file, so that it's logged to dev command timers
            let mut data_file = OpenOptions::new()
                .append(true)
                .open(&git_trace_file_path)
                .with_context(|| {
                    format!(
                        "Failed to open Git Trace2 file: {}",
                        git_trace_file_path.to_string_lossy()
                    )
                })?;

            // The telemetry binary will log lines that start with "error ",
            // so remove any newlines from the error message to avoid losing data.
            let full_log = format!(
                "error Push to large repo failed. \
                git_repo_name: {git_repo_name}. \
                large_repo_name: {large_repo_name}. \
                Error: {}",
                error.to_string().replace("\n", " ")
            );

            data_file.write(full_log.as_bytes()).with_context(|| {
                format!(
                    "Failed to write to Git Trace2 file: {}",
                    git_trace_file_path.to_string_lossy()
                )
            })?;
        }
        None => {
            bail!("Failed to get path of Git Trace2 file from GIT_TRACE2 environment variable")
        }
    }

    Ok(())
}
