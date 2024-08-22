/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[cfg(fbcode_build)]
use anyhow::Result;
use async_trait::async_trait;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use context::CoreContext;
use futures::join;
#[cfg(fbcode_build)]
use git_ref_rust_logger::GitRefLogger;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfigRef;
use git_source_of_truth::Staleness;
use gix_hash::Kind;
use gix_hash::ObjectId;
use hostname::get_hostname;
use logger_ext::Loggable;
use metaconfig_types::RepoConfigRef;
#[cfg(fbcode_build)]
use mononoke_bookmark_rust_logger::MononokeBookmarkLogger;
use mononoke_types::ChangesetId;
use repo_identity::RepoIdentityRef;
use serde_derive::Serialize;
#[cfg(fbcode_build)]
use whence_logged::WhenceScribeLogged;

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
    pub bookmark_name: BookmarkKey,
    pub bookmark_kind: BookmarkKind,
    pub operation: BookmarkOperation,
    pub reason: BookmarkUpdateReason,
}

#[derive(Serialize)]
struct GitBookmarkInfo {
    repo_name: String,
    bookmark_name: String,
    old_bookmark_value: String,
    new_bookmark_value: String,
    server_hostname: String,
    timestamp: u128,
}

impl GitBookmarkInfo {
    async fn new(
        ctx: &CoreContext,
        repo: &(impl RepoIdentityRef + BonsaiGitMappingRef),
        info: &BookmarkInfo,
    ) -> Self {
        let repo_name = format!("{}.git", repo.repo_identity().name());
        // Need to prepend the refs/ prefix since Mononoke bookmarks strip that off
        let bookmark_name = format!("refs/{}", &info.bookmark_name);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time travel exception!")
            .as_millis();
        let old_bookmark_value = get_git_hash(ctx, repo, info.operation.old_bookmark_value())
            .await
            .unwrap_or_else(|| ObjectId::null(Kind::Sha1).to_hex().to_string());
        let new_bookmark_value = get_git_hash(ctx, repo, info.operation.new_bookmark_value())
            .await
            .unwrap_or_else(|| ObjectId::null(Kind::Sha1).to_hex().to_string());
        let server_hostname = get_hostname().unwrap_or("error".to_string());
        Self {
            repo_name,
            bookmark_name,
            old_bookmark_value,
            new_bookmark_value,
            server_hostname,
            timestamp,
        }
    }
}

#[async_trait]
impl Loggable for GitBookmarkInfo {
    #[cfg(fbcode_build)]
    async fn log_to_logger(&self, ctx: &CoreContext) -> Result<()> {
        // Without override, WhenceScribeLogged is set to default which will cause
        // data being logged to "/sandbox" category if service is run from devserver.
        // But currently we use Logger only if we're in prod (as config implies), so
        // we should log to prod too, even from devserver.
        // For example, we can land a commit to prod from devserver, and logging for
        // this commit should go to prod, not to sandbox.
        GitRefLogger::override_whence_scribe_logged(ctx.fb, WhenceScribeLogged::PROD);
        let mut ref_logger = GitRefLogger::new(ctx.fb);
        ref_logger.set_repo_name(self.repo_name.clone());
        ref_logger.set_ref_name(self.bookmark_name.clone());
        ref_logger.set_old_ref_value(self.old_bookmark_value.clone());
        ref_logger.set_new_ref_value(self.new_bookmark_value.clone());
        ref_logger.set_pusher_identities(vec![]); // Maintaining parity with current Git logger
        ref_logger.set_server_hostname(self.server_hostname.clone());
        ref_logger.set_received_timestamp(self.timestamp as i64);
        ref_logger.log_async()?;
        Ok(())
    }
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

#[async_trait]
impl Loggable for PlainBookmarkInfo {
    #[cfg(fbcode_build)]
    async fn log_to_logger(&self, ctx: &CoreContext) -> Result<()> {
        // Without override, WhenceScribeLogged is set to default which will cause
        // data being logged to "/sandbox" category if service is run from devserver.
        // But currently we use Logger only if we're in prod (as config implies), so
        // we should log to prod too, even from devserver.
        // For example, we can land a commit to prod from devserver, and logging for
        // this commit should go to prod, not to sandbox.
        MononokeBookmarkLogger::override_whence_scribe_logged(ctx.fb, WhenceScribeLogged::PROD);
        let mut logger = MononokeBookmarkLogger::new(ctx.fb);
        logger
            .set_repo_name(self.repo_name.clone())
            .set_bookmark_name(self.bookmark_name.clone())
            .set_bookmark_kind(self.bookmark_kind.clone());
        if let Some(v) = &self.old_bookmark_value {
            logger.set_old_bookmark_value(v.clone());
        }
        if let Some(v) = &self.new_bookmark_value {
            logger.set_new_bookmark_value(v.clone());
        }
        logger
            .set_operation(self.operation.clone())
            .set_update_reason(self.update_reason.clone());

        if let Some(cri) = ctx.client_request_info() {
            logger.set_client_correlator(cri.correlator.clone());
            logger.set_client_entry_point(cri.entry_point.to_string());
        }
        logger.attach_raw_scribe_write_cat()?;
        logger.log_async()?;

        Ok(())
    }
}

pub async fn log_bookmark_operation(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + RepoConfigRef + BonsaiGitMappingRef + GitSourceOfTruthConfigRef),
    info: &BookmarkInfo,
) {
    if let Some(bookmark_logging_destination) = &repo
        .repo_config()
        .update_logging_config
        .bookmark_logging_destination
    {
        let plain_logger_future = async move {
            PlainBookmarkInfo::new(repo, info)
                .log(ctx, bookmark_logging_destination)
                .await;
        };
        let git_logger_future = async move {
            let mononoke_source_of_truth = repo
                .git_source_of_truth_config()
                .get_by_repo_id(ctx, repo.repo_identity().id(), Staleness::MaybeStale)
                .await
                .map(|entry| {
                    entry.map_or(false, |entry| {
                        entry.source_of_truth == GitSourceOfTruth::Mononoke
                    })
                })
                .unwrap_or(false);
            // Only log Git bookmarks if the Git repo is SoT'd in Mononoke
            if mononoke_source_of_truth {
                GitBookmarkInfo::new(ctx, repo, info)
                    .await
                    .log(ctx, bookmark_logging_destination)
                    .await;
            }
        };
        join!(plain_logger_future, git_logger_future);
    }
}

async fn get_git_hash(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + BonsaiGitMappingRef),
    maybe_cs_id: Option<ChangesetId>,
) -> Option<String> {
    if let Some(cs_id) = maybe_cs_id {
        repo.bonsai_git_mapping()
            .get_git_sha1_from_bonsai(ctx, cs_id)
            .await
            .unwrap_or(None)
            .map(|sha1| sha1.to_hex().to_string())
    } else {
        None
    }
}
