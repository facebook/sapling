/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changeset_info::ChangesetInfo;
use commit_cloud_types::LocalBookmarksMap;
use commit_cloud_types::RemoteBookmarksMap;
use commit_cloud_types::SmartlogFlag;
use commit_cloud_types::SmartlogNode;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::WorkspaceRemoteBookmark;
use mercurial_types::HgChangesetId;

use crate::ctx::CommitCloudContext;
use crate::sql::ops::Get;
use crate::sql::ops::GetAsMap;
use crate::CommitCloud;
use crate::Phase;
use crate::SqlCommitCloud;

// Workspace information needed to create smartlog
#[derive(Debug, Clone)]
pub struct RawSmartlogData {
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: LocalBookmarksMap,
    pub remote_bookmarks: RemoteBookmarksMap,
}
impl RawSmartlogData {
    // Takes all the heads and bookmarks and returns them as a single Vec<HgChangesetId>
    // in order to create a  smartlog node list
    pub fn collapse_into_vec(&self, flags: &[SmartlogFlag]) -> Vec<HgChangesetId> {
        let mut heads = self
            .heads
            .clone()
            .into_iter()
            .map(|head| head.commit)
            .collect::<Vec<HgChangesetId>>();

        if flags.contains(&SmartlogFlag::AddRemoteBookmarks) {
            let mut rbs = self
                .remote_bookmarks
                .keys()
                .cloned()
                .collect::<Vec<HgChangesetId>>();
            heads.append(&mut rbs);
        }

        if flags.contains(&SmartlogFlag::AddAllBookmarks) {
            let mut lbs = self
                .local_bookmarks
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
    ) -> Result<Self, anyhow::Error> {
        let heads: Vec<WorkspaceHead> =
            sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

        let local_bookmarks = sql
            .get_as_map(ctx.reponame.clone(), ctx.workspace.clone())
            .await?;

        let remote_bookmarks = sql
            .get_as_map(ctx.reponame.clone(), ctx.workspace.clone())
            .await?;

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
    ) -> anyhow::Result<RawSmartlogData> {
        RawSmartlogData::fetch_smartlog_references(
            &CommitCloudContext::new(&cc_ctx.workspace, &cc_ctx.reponame)?,
            &self.storage,
        )
        .await
    }

    pub fn make_smartlog_node(
        &self,
        hgid: &HgChangesetId,
        parents: &Vec<HgChangesetId>,
        node: &ChangesetInfo,
        local_bookmarks: &Option<Vec<String>>,
        remote_bookmarks: &Option<Vec<WorkspaceRemoteBookmark>>,
        phase: &Phase,
    ) -> anyhow::Result<SmartlogNode> {
        let author = node.author();
        let date = node.author_date().as_chrono().timestamp();
        let message = node.message();

        let node = SmartlogNode {
            node: *hgid,
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
