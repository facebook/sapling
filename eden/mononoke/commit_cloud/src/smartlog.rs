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
use crate::references::RawSmartlogData;
use crate::CommitCloud;
use crate::Phase;

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
