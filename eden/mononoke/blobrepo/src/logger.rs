/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::anyhow;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkName;
use context::CoreContext;
use metaconfig_types::RepoConfigRef;
#[cfg(fbcode_build)]
use mononoke_bookmark_rust_logger::MononokeBookmarkLogger;
use mononoke_types::ChangesetId;
use repo_identity::RepoIdentityRef;
use scribe_ext::Scribe;
use serde_derive::Serialize;

#[derive(Serialize)]
pub enum BookmarkOperation {
    Create(ChangesetId),
    Update(ChangesetId, ChangesetId),
    Pushrebase(Option<ChangesetId>, ChangesetId),
    Delete(ChangesetId),
}

impl std::fmt::Display for BookmarkOperation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use BookmarkOperation::*;

        let s = match self {
            Create(_) => "create",
            Update(_, _) => "update",
            Pushrebase(_, _) => "pushrebase",
            Delete(_) => "delete",
        };

        write!(f, "{}", s)
    }
}

impl BookmarkOperation {
    fn old_bookmark_value(&self) -> Option<ChangesetId> {
        use BookmarkOperation::*;
        match *self {
            Create(_) => None,
            Update(old, _) => Some(old),
            Pushrebase(old, _) => old,
            Delete(old) => Some(old),
        }
    }
    fn new_bookmark_value(&self) -> Option<ChangesetId> {
        use BookmarkOperation::*;
        match *self {
            Create(new) => Some(new),
            Update(_, new) => Some(new),
            Pushrebase(_, new) => Some(new),
            Delete(_) => None,
        }
    }
}

pub struct BookmarkInfo {
    pub bookmark_name: BookmarkName,
    pub bookmark_kind: BookmarkKind,
    pub operation: BookmarkOperation,
    pub reason: BookmarkUpdateReason,
}

#[derive(Serialize)]
struct PlainBookmarkInfo {
    repo_name: String,
    bookmark_name: String,
    bookmark_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_bookmark_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_bookmark_value: Option<String>,
    operation: String,
    update_reason: String,
}

impl PlainBookmarkInfo {
    fn new(repo: &impl RepoIdentityRef, info: &BookmarkInfo) -> Self {
        Self {
            repo_name: repo.repo_identity().name().to_owned(),
            bookmark_name: format!("{}", info.bookmark_name),
            bookmark_kind: format!("{}", info.bookmark_kind),
            old_bookmark_value: info
                .operation
                .old_bookmark_value()
                .map(|cs| format!("{}", cs)),
            new_bookmark_value: info
                .operation
                .new_bookmark_value()
                .map(|cs| format!("{}", cs)),
            operation: format!("{}", info.operation),
            update_reason: format!("{}", info.reason),
        }
    }
}

async fn log_bookmark_operation_to_raw_scribe(
    info: &PlainBookmarkInfo,
    repo: &impl RepoConfigRef,
    scribe: &Scribe,
) -> anyhow::Result<()> {
    if let Some(category) = &repo.repo_config().bookmark_scribe_category {
        scribe
            .offer(
                category,
                &serde_json::to_string(&info).map_err(|e| anyhow!("{}", e))?,
            )
            .map_err(|e| anyhow!("{}", e))?;
    }
    Ok(())
}

#[cfg(fbcode_build)]
async fn log_bookmark_operation_to_file_if_appropriate(
    info: &PlainBookmarkInfo,
    repo: &impl RepoConfigRef,
    scribe: &Scribe,
) -> anyhow::Result<()> {
    if let Scribe::LogToFile(_) = scribe {
        log_bookmark_operation_to_raw_scribe(info, repo, scribe).await?;
    }
    Ok(())
}

#[cfg(fbcode_build)]
async fn log_bookmark_operation_to_logger(
    info: &PlainBookmarkInfo,
    ctx: &CoreContext,
) -> anyhow::Result<()> {
    let mut logger = MononokeBookmarkLogger::new(ctx.fb);
    logger
        .set_repo_name(info.repo_name.clone())
        .set_bookmark_name(info.bookmark_name.clone())
        .set_bookmark_kind(info.bookmark_kind.clone());
    if let Some(v) = &info.old_bookmark_value {
        logger.set_old_bookmark_value(v.clone());
    }
    if let Some(v) = &info.new_bookmark_value {
        logger.set_new_bookmark_value(v.clone());
    }
    logger
        .set_operation(info.operation.clone())
        .set_update_reason(info.update_reason.clone());

    logger.log_async().map_err(|err| anyhow!("{}", err))
}

pub async fn log_bookmark_operation(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + RepoConfigRef),
    info: &BookmarkInfo,
) {
    let data = PlainBookmarkInfo::new(repo, info);
    #[cfg(fbcode_build)]
    {
        let res = log_bookmark_operation_to_logger(&data, ctx).await;
        if let Err(err) = res {
            ctx.scuba().clone().log_with_msg(
                "Failed to log bookmark operation to logger",
                Some(format!("{}", err)),
            );
        } else {
            // Logger doesn't respect Mononoke's LogToFile scribe setting,
            // so when scribe is set to log to file, issue a raw scribe call.
            // This allows us to write integration tests that can read the file
            // content to be sure that the data is and remain correct-looking.
            log_bookmark_operation_to_file_if_appropriate(&data, repo, ctx.scribe())
                .await
                .unwrap_or_else(|err| {
                    ctx.scuba().clone().log_with_msg(
                        "Failed to log bookmark operation to file",
                        Some(format!("{}", err)),
                    );
                });
        }
    }
    // When building in oss, we don't have access to the logger framework, so fallback to raw
    // scribe call.
    #[cfg(not(fbcode_build))]
    {
        log_bookmark_operation_to_raw_scribe(&data, repo, ctx.scribe())
            .await
            .unwrap_or_else(|err| {
                ctx.scuba().clone().log_with_msg(
                    "Failed to log bookmark operation to scribe",
                    Some(format!("{}", err)),
                );
            });
    }
}
