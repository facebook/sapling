/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changeset_info::ChangesetInfo;
use edenapi_types::cloud::RemoteBookmark;
use edenapi_types::GetSmartlogFlag;
use edenapi_types::HgId;
use edenapi_types::SmartlogNode;
use mercurial_types::HgChangesetId;

use crate::ctx::CommitCloudContext;
use crate::references::heads::WorkspaceHead;
use crate::references::local_bookmarks::LocalBookmarksMap;
use crate::references::remote_bookmarks::RemoteBookmarksMap;
use crate::sql::ops::Get;
use crate::sql::ops::GetAsMap;
use crate::CommitCloud;
use crate::Phase;
use crate::SqlCommitCloud;

// Workspace information needed to create smartlog
#[derive(Debug, Clone)]
pub struct RawSmartlogData {
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: Option<LocalBookmarksMap>,
    pub remote_bookmarks: Option<RemoteBookmarksMap>,
}
impl RawSmartlogData {
    // Takes all the heads and bookmarks and returns them as a single Vec<HgChangesetId>
    // in order to create a  smartlog node list
    pub fn collapse_into_vec(&self) -> Vec<HgChangesetId> {
        let mut heads = self
            .heads
            .clone()
            .into_iter()
            .map(|head| head.commit)
            .collect::<Vec<HgChangesetId>>();

        if let Some(remote_bookmarks) = self.remote_bookmarks.clone() {
            let mut rbs = remote_bookmarks
                .keys()
                .cloned()
                .collect::<Vec<HgChangesetId>>();
            heads.append(&mut rbs);
        }

        if let Some(local_bookmarks) = self.local_bookmarks.clone() {
            let mut lbs = local_bookmarks
                .keys()
                .cloned()
                .collect::<Vec<HgChangesetId>>();
            heads.append(&mut lbs);
        }
        heads
    }

    pub(crate) async fn fetch_smartlog_references(
        ctx: &CommitCloudContext,
        sql: &SqlCommitCloud,
        flags: &[GetSmartlogFlag],
    ) -> Result<Self, anyhow::Error> {
        let heads: Vec<WorkspaceHead> =
            sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

        let local_bookmarks = if flags.contains(&GetSmartlogFlag::AddAllBookmarks) {
            Some(
                sql.get_as_map(ctx.reponame.clone(), ctx.workspace.clone())
                    .await?,
            )
        } else {
            None
        };

        let remote_bookmarks = if flags.contains(&GetSmartlogFlag::AddRemoteBookmarks) {
            Some(
                sql.get_as_map(ctx.reponame.clone(), ctx.workspace.clone())
                    .await?,
            )
        } else {
            None
        };

        Ok(RawSmartlogData {
            heads,
            local_bookmarks,
            remote_bookmarks,
        })
    }
}

impl CommitCloud {
    pub async fn get_smartlog_raw_info(
        &self,
        cc_ctx: &CommitCloudContext,
        flags: &[GetSmartlogFlag],
    ) -> anyhow::Result<RawSmartlogData> {
        RawSmartlogData::fetch_smartlog_references(
            &CommitCloudContext::new(&cc_ctx.workspace, &cc_ctx.reponame)?,
            &self.storage,
            flags,
        )
        .await
    }

    pub fn make_smartlog_node(
        &self,
        hgid: &HgChangesetId,
        parents: &Vec<HgId>,
        node: &ChangesetInfo,
        local_bookmarks: &Option<Vec<String>>,
        remote_bookmarks: &Option<Vec<RemoteBookmark>>,
        phase: &Phase,
    ) -> anyhow::Result<SmartlogNode> {
        let author = node.author();
        let date = node.author_date().as_chrono().timestamp();
        let message = node.message();

        let node = SmartlogNode {
            node: (*hgid).into(),
            phase: phase.to_string(),
            author: author.to_string(),
            date,
            message: message.to_string(),
            parents: parents.to_owned(),
            bookmarks: local_bookmarks.to_owned().unwrap_or_default(),
            remote_bookmarks: remote_bookmarks.to_owned(),
        };
        Ok(node)
    }
}
