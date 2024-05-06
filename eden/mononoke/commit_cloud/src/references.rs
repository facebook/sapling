/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use edenapi_types::cloud::RemoteBookmark;
use edenapi_types::HgId;
use edenapi_types::ReferencesData;
use mononoke_types::Timestamp;

use crate::sql::heads::WorkspaceHead;
use crate::sql::local_bookmarks::WorkspaceLocalBookmark;
use crate::sql::ops::Get;
use crate::sql::ops::SqlCommitCloud;
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
    let mut heads: Vec<HgId> = Vec::new();
    let mut bookmarks: HashMap<String, HgId> = HashMap::new();
    let mut heads_dates: HashMap<HgId, i64> = HashMap::new();
    let mut remote_bookmarks: Vec<RemoteBookmark> = Vec::new();
    let mut snapshots: Vec<HgId> = Vec::new();
    let _timestamp: i64 = 0;

    for head in raw_references_data.heads {
        heads.push(head.commit.into());
        // TODO: Retrieve this information from SCS.
        heads_dates.insert(head.commit.into(), Timestamp::now().timestamp_nanos());
    }
    for bookmark in raw_references_data.local_bookmarks {
        bookmarks.insert(bookmark.name.clone(), bookmark.commit.into());
    }

    for remote_bookmark in raw_references_data.remote_bookmarks {
        remote_bookmarks.push(RemoteBookmark {
            remote: remote_bookmark.remote.clone(),
            name: remote_bookmark.name.clone(),
            node: Some(remote_bookmark.commit.into()),
        });
    }

    for snapshot in raw_references_data.snapshots {
        snapshots.push(snapshot.commit.into());
    }

    Ok(ReferencesData {
        version: latest_version,
        heads: Some(heads),
        bookmarks: Some(bookmarks),
        heads_dates: Some(heads_dates),
        remote_bookmarks: Some(remote_bookmarks),
        snapshots: Some(snapshots),
        timestamp: Some(version_timestamp),
    })
}
