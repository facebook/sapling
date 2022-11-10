/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::anyhow;
use anyhow::Result;
use bookmarks_types::BookmarkKind;
use bookmarks_types::BookmarkName;
use changesets::ChangesetsRef;
use chrono::DateTime;
use chrono::Utc;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use once_cell::sync::Lazy;
use permission_checker::MononokeIdentitySet;
use regex::Regex;
use repo_identity::RepoIdentityRef;
use serde_derive::Serialize;

pub struct CommitInfo {
    changeset_id: ChangesetId,
    bubble_id: Option<NonZeroU64>,
    diff_id: Option<String>,
    changed_files_info: ChangedFilesInfo,
}

impl CommitInfo {
    pub fn new(bcs: &BonsaiChangeset, bubble_id: Option<BubbleId>) -> Self {
        CommitInfo {
            changeset_id: bcs.get_changeset_id(),
            bubble_id: bubble_id.map(Into::into),
            diff_id: extract_differential_revision(bcs.message()).map(ToString::to_string),
            changed_files_info: ChangedFilesInfo::new(bcs),
        }
    }
}

fn extract_differential_revision(message: &str) -> Option<&str> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?m)^Differential Revision: [^\n]*/D([0-9]+)")
            .expect("Failed to compile differential revision regex")
    });

    Some(RE.captures(message)?.get(1)?.as_str())
}

pub struct ChangedFilesInfo {
    changed_files_count: u64,
    changed_files_size: u64,
}

impl ChangedFilesInfo {
    pub fn new(bcs: &BonsaiChangeset) -> Self {
        let changed_files_count = bcs.file_changes_map().len() as u64;
        let changed_files_size = bcs
            .file_changes_map()
            .values()
            .map(|fc| fc.size().unwrap_or(0))
            .sum::<u64>() as u64;

        Self {
            changed_files_count,
            changed_files_size,
        }
    }
}

#[derive(Serialize)]
struct PlainCommitInfo {
    // Repo ID is logged to legacy scuba for compatibility, but should be
    // considered deprecated and not logged to Logger.
    repo_id: i32,
    repo_name: String,
    is_public: bool,
    changeset_id: ChangesetId,
    #[serde(skip_serializing_if = "Option::is_none")]
    bubble_id: Option<NonZeroU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff_id: Option<String>,
    changed_files_count: u64,
    changed_files_size: u64,
    parents: Vec<ChangesetId>,
    generation: Generation,
    #[serde(skip_serializing_if = "Option::is_none")]
    bookmark: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_unix_name: Option<String>,
    #[serde(skip_serializing_if = "MononokeIdentitySet::is_empty")]
    user_identities: MononokeIdentitySet,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_hostname: Option<String>,
    #[serde(with = "::chrono::serde::ts_seconds")]
    received_timestamp: DateTime<Utc>,
}

impl PlainCommitInfo {
    async fn new(
        ctx: &CoreContext,
        repo: &(impl ChangesetsRef + RepoIdentityRef),
        received_timestamp: DateTime<Utc>,
        bookmark: Option<(&BookmarkName, BookmarkKind)>,
        commit_info: CommitInfo,
    ) -> Result<PlainCommitInfo> {
        let CommitInfo {
            changeset_id,
            bubble_id,
            diff_id,
            changed_files_info:
                ChangedFilesInfo {
                    changed_files_count,
                    changed_files_size,
                },
        } = commit_info;
        let repo_id = repo.repo_identity().id().id();
        let repo_name = repo.repo_identity().name().to_string();
        let cs = repo
            .changesets()
            .get(ctx.clone(), changeset_id)
            .await?
            .ok_or_else(|| anyhow!("Changeset not found: {}", changeset_id))?;
        let parents = cs.parents;
        let generation = Generation::new(cs.gen);
        let user_unix_name = ctx.metadata().unix_name().map(|un| un.to_string());
        let user_identities = ctx.metadata().identities().clone();
        let source_hostname = ctx.metadata().client_hostname().map(|hn| hn.to_string());
        let (bookmark, is_public) = bookmark.map_or((None, false), |(name, kind)| {
            (Some(name.to_string()), kind.is_public())
        });

        Ok(PlainCommitInfo {
            repo_id,
            repo_name,
            is_public,
            changeset_id,
            bubble_id,
            diff_id,
            changed_files_count,
            changed_files_size,
            parents,
            generation,
            bookmark,
            user_unix_name,
            user_identities,
            source_hostname,
            received_timestamp,
        })
    }
}

pub async fn log_new_commits(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + ChangesetsRef + RepoConfigRef),
    bookmark: Option<(&BookmarkName, BookmarkKind)>,
    commit_infos: Vec<CommitInfo>,
) {
    let is_public = bookmark.map_or(false, |(_, kind)| kind.is_public());
    let legacy_category = if is_public {
        repo.repo_config()
            .pushrebase
            .commit_scribe_category
            .as_deref()
    } else {
        repo.repo_config()
            .infinitepush
            .commit_scribe_category
            .as_deref()
    };
    let scribe = ctx.scribe();

    // If nothing is going to be logged, we can exit early.
    if legacy_category.is_none() {
        return;
    }

    let received_timestamp = Utc::now();

    let res = stream::iter(commit_infos)
        .map(Ok)
        .try_for_each_concurrent(100, |commit_info| async move {
            let plain_commit_info =
                PlainCommitInfo::new(ctx, repo, received_timestamp, bookmark, commit_info).await?;
            if let Some(category) = legacy_category {
                scribe.offer(category, &serde_json::to_string(&plain_commit_info)?)?;
            }
            anyhow::Ok(())
        })
        .await;

    if let Err(err) = res {
        ctx.scuba().clone().log_with_msg(
            "Failed to log new draft commit to scribe",
            Some(err.to_string()),
        );
    }
}
