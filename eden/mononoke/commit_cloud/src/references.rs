/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use mononoke_types::Timestamp;

use crate::sql::heads::WorkspaceHead;
use crate::sql::local_bookmarks::WorkspaceLocalBookmark;
use crate::sql::ops::Get;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::remote_bookmarks::RefRemoteBookmark;
use crate::sql::remote_bookmarks::WorkspaceRemoteBookmark;
use crate::sql::snapshots::WorkspaceSnapshot;
use crate::CommitCloudContext;

// Workspace information as we retrieve it form the database
#[derive(Debug, Clone)]
pub struct RawReferencesData {
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: Vec<WorkspaceLocalBookmark>,
    pub remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
    pub snapshots: Vec<WorkspaceSnapshot>,
}

// Workspace information in the format the client expects it
#[derive(Debug, Clone, Default)]
pub struct ReferencesData {
    pub version: i64,
    pub heads: Option<Vec<String>>,
    pub bookmarks: Option<HashMap<String, String>>,
    pub heads_dates: Option<HashMap<String, i64>>,
    pub remote_bookmarks: Option<Vec<RefRemoteBookmark>>,
    pub snapshots: Option<Vec<String>>,
    pub timestamp: Option<i64>,
}

// Perform all get queries into the database
pub(crate) async fn fetch_references(
    ctx: CommitCloudContext,
    sql: &SqlCommitCloud,
) -> Result<RawReferencesData, anyhow::Error> {
    let heads: Vec<WorkspaceHead> = sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    let local_bookmarks: Vec<WorkspaceLocalBookmark> =
        sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    let remote_bookmarks: Vec<WorkspaceRemoteBookmark> =
        sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    let snapshots: Vec<WorkspaceSnapshot> =
        sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    Ok(RawReferencesData {
        heads,
        local_bookmarks,
        remote_bookmarks,
        snapshots,
    })
}

// Cast the raw data into the format the client expects it
pub(crate) async fn cast_references_data(
    raw_references_data: RawReferencesData,
    latest_version: u64,
    version_timestamp: i64,
) -> Result<ReferencesData, anyhow::Error> {
    let mut heads: Vec<String> = Vec::new();
    let mut bookmarks: HashMap<String, String> = HashMap::new();
    let mut heads_dates: HashMap<String, i64> = HashMap::new();
    let mut remote_bookmarks: Vec<RefRemoteBookmark> = Vec::new();
    let mut snapshots: Vec<String> = Vec::new();
    let _timestamp: i64 = 0;

    for head in raw_references_data.heads {
        heads.push(head.commit.to_string());
        // TODO: Retrieve this information from SCS.
        heads_dates.insert(head.commit.to_string(), Timestamp::now().timestamp_nanos());
    }
    for bookmark in raw_references_data.local_bookmarks {
        bookmarks.insert(bookmark.name.clone(), bookmark.commit.to_string());
    }

    for remote_bookmark in raw_references_data.remote_bookmarks {
        remote_bookmarks.push(RefRemoteBookmark {
            remote: remote_bookmark.remote.clone(),
            name: remote_bookmark.name.clone(),
            node: Some(remote_bookmark.commit.to_string()),
        });
    }

    for snapshot in raw_references_data.snapshots {
        snapshots.push(snapshot.commit.to_string());
    }

    Ok(ReferencesData {
        version: latest_version as i64,
        heads: Some(heads),
        bookmarks: Some(bookmarks),
        heads_dates: Some(heads_dates),
        remote_bookmarks: Some(remote_bookmarks),
        snapshots: Some(snapshots),
        timestamp: Some(version_timestamp),
    })
}
