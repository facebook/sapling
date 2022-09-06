/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(hash_drain_filter)]
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use facebook::*;
#[cfg(not(fbcode_build))]
pub use oss::*;

const ENCODED_SLASH: &str = "_SLASH_";
const ENCODED_PLUS: &str = "_PLUS_";
const X_REPO_SEPARATOR: &str = "_TO_";

/// Trait outlining the method responsible for performing the initial bootstrapping
/// of the process with the context of the incoming repo. Implementer of this trait should
/// NOT contain any repo-specific state since one instance of RepoShardedProcess
/// caters all repos for the given job.
#[async_trait]
pub trait RepoShardedProcess: Send + Sync {
    /// Method responsible for performing the initial setup of the process in context of
    /// the provided repo. This method should ONLY contain code necessary to build
    /// state (in form of the struct that implements RepoShardedProcessExecutor trait)
    /// that is required to execute the job. The repo-name (or related entity)
    /// should be included as part of the RepoShardedProcessExecutor state.
    async fn setup(&self, repo_name: &str) -> Result<Arc<dyn RepoShardedProcessExecutor>>;
}

/// Trait outlining the methods to be implemented by Mononoke jobs that require
/// sharding across different repos based on their resource requirements.
/// Implementer of this trait can store repo-specific state if required since there
/// exists a 1-1 mapping between a repo and a RepoShardedProcessExecutor.
#[async_trait]
pub trait RepoShardedProcessExecutor: Send + Sync {
    /// Callback for when the repo-process for the current job is ready to begin execution
    /// with the provided repo. The core process logic should exist within this method.
    /// The process execution can be one-shot, batched or long-running in nature.
    /// Correspondingly, the task associated with this method could return immediately,
    /// or after a while or just keep executing until the executor decides to terminate it.
    /// Depending on the execution mode, the implementer can choose to maintain some
    /// form of state (i.e. signal, oneshot, etc.) that can be used to interrupt the
    /// normal execution of the process when the Sharded Process Manager needs to move
    /// or terminate the process execution for the provided repo.
    async fn execute(&self) -> Result<()>;

    /// Callback for when the executing repo process for the current job is required to
    /// relinquish an existing repo assigned to it earlier. The timeout specified during
    /// creation determines the time for which the executing repo process will wait before
    /// performing a hard-cleaup of the repo associated task. Note that this
    /// method is responsible ONLY for signalling the termination of the execution
    /// of the repo for the given process. Once the control returns from this method,
    /// the executor waits for timeout seconds (specified during executor construction)
    /// before initiating forced termination of the executing process. If there
    /// are any book-keeping activities pending, they should be completed in the
    /// main execution method during this timeout period. The stop() method itself
    /// SHOULD return quickly (i.e. should not be long running).
    async fn stop(&self) -> Result<()>;
}

/// Function responsible for decoding an SM-encoded repo-name.
pub fn decode_repo_name(encoded_repo_name: &str) -> String {
    encoded_repo_name
        .replace(ENCODED_SLASH, "/")
        .replace(ENCODED_PLUS, "+")
}

/// Function responsible for splitting source and target repo name
/// from combined repo-name string.
pub fn split_repo_names(combined_repo_names: &str) -> Vec<&str> {
    combined_repo_names.split(X_REPO_SEPARATOR).collect()
}
