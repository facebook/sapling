/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use std::fmt;
use std::time::Duration;
use tunables::tunables;

const SCUBA_TABLE: &str = "mononoke_x_repo_mapping";

const SOURCE_REPO: &str = "source_repo";
const TARGET_REPO: &str = "target_repo";
const SOURCE_CS_ID: &str = "source_cs_id";
const SYNC_FN: &str = "sync_fn";
const SYNC_CONTEXT: &str = "sync_context";
const TARGET_CS_ID: &str = "target_cs_id";
const DURATION_MS: &str = "duration_ms";
const ERROR: &str = "error";
const SUCCESS: &str = "success";
const SESSION_ID: &str = "session_id";

/// Context of a commit sync function being called
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CommitSyncContext {
    AdminChangeMapping,
    Backsyncer,
    BacksyncerChangeMapping,
    ManualCommitSync,
    PushRedirector,
    RepoImport,
    ScsXrepoLookup,
    SyncDiamondMerge,
    Tests,
    Unknown,
    XRepoSyncJob,
}

impl fmt::Display for CommitSyncContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdminChangeMapping => write!(f, "admin-change-mapping"),
            Self::Backsyncer => write!(f, "backsyncer"),
            Self::BacksyncerChangeMapping => write!(f, "backsyncer-change-mapping"),
            Self::ManualCommitSync => write!(f, "manual-commit-sync"),
            Self::PushRedirector => write!(f, "push-redirector"),
            Self::RepoImport => write!(f, "repo-import"),
            Self::ScsXrepoLookup => write!(f, "scs-xrepo-lookup"),
            Self::SyncDiamondMerge => write!(f, "sync-diamond-merge"),
            Self::Tests => write!(f, "tests"),
            Self::Unknown => write!(f, "unknown"),
            Self::XRepoSyncJob => write!(f, "x-repo-sync-job"),
        }
    }
}

pub fn log_rewrite(
    ctx: &CoreContext,
    mut sample: MononokeScubaSampleBuilder,
    source_cs_id: ChangesetId,
    sync_fn: &str,
    commit_sync_context: CommitSyncContext,
    duration: Duration,
    sync_result: &Result<Option<ChangesetId>, Error>,
) {
    if !tunables().get_enable_logging_commit_rewrite_data() {
        return;
    }

    sample
        .add(DURATION_MS, duration.as_millis() as u64)
        .add(SOURCE_CS_ID, format!("{}", source_cs_id))
        .add(SYNC_FN, sync_fn)
        .add(
            SESSION_ID,
            format!("session {}", ctx.metadata().session_id()),
        )
        .add(SYNC_CONTEXT, format!("{}", commit_sync_context));

    match sync_result {
        Ok(maybe_target_cs_id) => {
            sample.add(SUCCESS, 1);
            if let Some(target_cs_id) = maybe_target_cs_id {
                sample.add(TARGET_CS_ID, format!("{}", target_cs_id));
            }
        }
        Err(e) => {
            sample.add(SUCCESS, 0).add(ERROR, format!("{}", e));
        }
    }

    sample.log();
}

pub fn get_scuba_sample(
    ctx: &CoreContext,
    source_repo: impl AsRef<str>,
    target_repo: impl AsRef<str>,
) -> MononokeScubaSampleBuilder {
    let mut scuba_sample = MononokeScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE);
    scuba_sample
        .add_common_server_data()
        .add(SOURCE_REPO, source_repo.as_ref().to_string())
        .add(TARGET_REPO, target_repo.as_ref().to_string());

    scuba_sample
}
