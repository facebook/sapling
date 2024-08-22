/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use edenapi_types::cloud::HistoricalVersion;
use edenapi_types::cloud::HistoricalVersionsData;
use edenapi_types::cloud::RemoteBookmark;
use mercurial_types::HgChangesetId;
use mononoke_types::Timestamp;

use super::local_bookmarks::LocalBookmarksMap;
use super::remote_bookmarks::RemoteBookmarksMap;
use super::RawReferencesData;
use crate::references::heads::WorkspaceHead;
use crate::references::local_bookmarks::WorkspaceLocalBookmark;
use crate::references::remote_bookmarks::WorkspaceRemoteBookmark;
use crate::sql::history_ops::GetOutput;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceHistory {
    pub version: u64,
    pub timestamp: Option<Timestamp>,
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: Vec<WorkspaceLocalBookmark>,
    pub remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
}

impl WorkspaceHistory {
    pub fn from_references(
        raw: RawReferencesData,
        version: u64,
        timestamp: i64,
    ) -> WorkspaceHistory {
        Self {
            version,
            timestamp: if timestamp == 0 {
                None
            } else {
                Some(Timestamp::from_timestamp_secs(timestamp))
            },
            heads: raw.heads,
            local_bookmarks: raw.local_bookmarks,
            remote_bookmarks: raw.remote_bookmarks,
        }
    }

    pub fn local_bookmarks_as_map(&self) -> LocalBookmarksMap {
        let mut map = LocalBookmarksMap::new();
        self.local_bookmarks.iter().for_each(|bookmark| {
            let value = map.entry(*bookmark.commit()).or_insert(vec![]);
            value.push(bookmark.name().clone())
        });
        map
    }

    pub fn remote_bookmarks_as_map(&self) -> RemoteBookmarksMap {
        let mut map = RemoteBookmarksMap::new();
        self.remote_bookmarks.iter().for_each(|bookmark| {
            let rb = RemoteBookmark {
                name: bookmark.name().clone(),
                node: Some((*bookmark.commit()).into()),
                remote: bookmark.remote().clone(),
            };
            let value = map.entry(*bookmark.commit()).or_insert(vec![]);
            value.push(rb)
        });
        map
    }

    // Takes all the heads and bookmarks and returns them as a single Vec<HgChangesetId>
    // in order to create a  smartlog node list
    pub fn collapse_into_vec(
        &self,
        rbs: &RemoteBookmarksMap,
        lbs: &LocalBookmarksMap,
    ) -> Vec<HgChangesetId> {
        self.heads
            .clone()
            .into_iter()
            .map(|head| head.commit)
            .chain(rbs.keys().cloned())
            .chain(lbs.keys().cloned())
            .collect::<Vec<HgChangesetId>>()
    }
}

pub fn historical_versions_from_get_output(
    output: Vec<GetOutput>,
) -> anyhow::Result<HistoricalVersionsData> {
    let mut versions = Vec::new();
    for out in output {
        match out {
            GetOutput::VersionTimestamp((version, timestamp)) => {
                versions.push(HistoricalVersion {
                    version_number: version as i64,
                    timestamp: timestamp.timestamp_seconds(),
                });
            }
            _ => bail!("'historical_data' failed: expected output from get_version_timestamp"),
        }
    }
    Ok(HistoricalVersionsData { versions })
}
