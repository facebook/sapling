/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::anyhow;
use bookmarks_types::BookmarkName;
use changesets::ChangesetsRef;
use chrono::Utc;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use repo_identity::RepoIdentityRef;
use scribe_commit_queue::ChangedFilesInfo;
use scribe_commit_queue::CommitInfo;
use scribe_commit_queue::LogToScribe;

pub struct ScribeCommitInfo {
    pub changeset_id: ChangesetId,
    pub bubble_id: Option<NonZeroU64>,
    pub changed_files: ChangedFilesInfo,
}

pub async fn log_commits_to_scribe_raw(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + ChangesetsRef),
    bookmark: Option<&BookmarkName>,
    changesets_and_changed_files_count: Vec<ScribeCommitInfo>,
    commit_scribe_category: Option<&str>,
) {
    let queue = match commit_scribe_category {
        Some(category) if !category.is_empty() => {
            LogToScribe::new(ctx.scribe().clone(), category.to_string())
        }
        _ => LogToScribe::new_with_discard(),
    };

    let repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name();
    let bookmark = bookmark.map(|bm| bm.as_str());
    let received_timestamp = Utc::now();

    let res = stream::iter(changesets_and_changed_files_count)
        .map(Ok)
        .map_ok(
            |ScribeCommitInfo {
                 changeset_id,
                 bubble_id,
                 changed_files,
             }| {
                let queue = &queue;
                async move {
                    let cs = repo
                        .changesets()
                        .get(ctx.clone(), changeset_id)
                        .await?
                        .ok_or_else(|| anyhow!("Changeset not found: {}", changeset_id))?;
                    let generation = Generation::new(cs.gen);
                    let parents = cs.parents;

                    let username = ctx.metadata().unix_name();
                    let hostname = ctx.metadata().client_hostname();
                    let identities = ctx.metadata().identities();
                    let ci = CommitInfo::new(
                        repo_id,
                        repo_name,
                        bookmark,
                        generation,
                        changeset_id,
                        bubble_id,
                        parents,
                        username,
                        identities,
                        hostname,
                        received_timestamp,
                        changed_files,
                    );
                    queue.queue_commit(&ci)
                }
            },
        )
        .try_for_each_concurrent(100, |f| f)
        .await;
    if let Err(err) = res {
        ctx.scuba()
            .clone()
            .log_with_msg("Failed to log pushed commits", Some(format!("{}", err)));
    }
}

pub async fn log_commit_to_scribe(
    ctx: &CoreContext,
    category: &str,
    container: &(impl RepoIdentityRef + ChangesetsRef),
    changeset: &BonsaiChangeset,
    bubble: Option<BubbleId>,
) {
    let changeset_id = changeset.get_changeset_id();
    let changed_files = ChangedFilesInfo::new(changeset);

    log_commits_to_scribe_raw(
        ctx,
        container,
        None,
        vec![ScribeCommitInfo {
            changeset_id,
            bubble_id: bubble.map(Into::into),
            changed_files,
        }],
        Some(category),
    )
    .await;
}
