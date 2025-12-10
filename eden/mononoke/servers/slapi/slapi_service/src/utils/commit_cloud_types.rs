/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use commit_cloud_types::ClientInfo as CloudClientInfo;
use commit_cloud_types::HistoricalVersion as CloudHistoricalVersion;
use commit_cloud_types::ReferencesData as CloudReferencesData;
use commit_cloud_types::SmartlogData as CloudSmartlogData;
use commit_cloud_types::SmartlogFilter as CloudSmartlogFilter;
use commit_cloud_types::SmartlogFlag;
use commit_cloud_types::SmartlogNode as CloudSmartlogNode;
use commit_cloud_types::UpdateReferencesParams as CloudUpdateReferencesParams;
use commit_cloud_types::WorkspaceData as CloudWorkspaceData;
use commit_cloud_types::WorkspaceRemoteBookmark;
use commit_cloud_types::WorkspaceSharingData as CloudWorkspaceSharingData;
use commit_cloud_types::changeset::CloudChangesetId;
use edenapi_types::GetSmartlogFlag;
use edenapi_types::HistoricalVersion;
use edenapi_types::SmartlogData;
use edenapi_types::SmartlogNode;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceData;
use edenapi_types::WorkspaceSharingData;
use edenapi_types::cloud::ClientInfo;
use edenapi_types::cloud::ReferencesData;
use edenapi_types::cloud::RemoteBookmark;
use edenapi_types::cloud::SmartlogFilter;
use mononoke_types::sha1_hash::Sha1;
use types::Id20;

pub trait FromCommitCloudType<T> {
    fn from_cc_type(cc: T) -> Result<Self>
    where
        Self: std::marker::Sized;
}

pub trait IntoCommitCloudType<T> {
    fn into_cc_type(self) -> Result<T>;
}

impl IntoCommitCloudType<CloudUpdateReferencesParams> for UpdateReferencesParams {
    fn into_cc_type(self) -> Result<CloudUpdateReferencesParams> {
        Ok(CloudUpdateReferencesParams {
            workspace: self.workspace,
            reponame: strip_git_suffix(&self.reponame).to_owned(),
            version: self.version,
            removed_heads: map_id_into_cloud_ids(self.removed_heads)?,
            new_heads: map_id_into_cloud_ids(self.new_heads)?,
            updated_bookmarks: self
                .updated_bookmarks
                .into_iter()
                .map(|(name, node)| {
                    (
                        name,
                        CloudChangesetId(Sha1::from_byte_array(node.into_byte_array())),
                    )
                })
                .collect(),
            removed_bookmarks: self.removed_bookmarks,
            updated_remote_bookmarks: self
                .updated_remote_bookmarks
                .map(rbs_into_cc_type)
                .transpose()?,
            removed_remote_bookmarks: self
                .removed_remote_bookmarks
                .map(rbs_into_cc_type)
                .transpose()?,
            new_snapshots: map_id_into_cloud_ids(self.new_snapshots)?,
            removed_snapshots: map_id_into_cloud_ids(self.removed_snapshots)?,
            client_info: self.client_info.map(|ci| ci.into_cc_type()).transpose()?,
        })
    }
}

impl IntoCommitCloudType<CloudClientInfo> for ClientInfo {
    fn into_cc_type(self) -> Result<CloudClientInfo> {
        Ok(CloudClientInfo {
            hostname: self.hostname,
            version: self.version,
            reporoot: self.reporoot,
        })
    }
}

impl IntoCommitCloudType<WorkspaceRemoteBookmark> for RemoteBookmark {
    fn into_cc_type(self) -> Result<WorkspaceRemoteBookmark> {
        WorkspaceRemoteBookmark::new(
            self.remote,
            self.name,
            CloudChangesetId(Sha1::from_byte_array(
                self.node.unwrap_or_default().into_byte_array(),
            )),
        )
    }
}

impl IntoCommitCloudType<SmartlogFlag> for GetSmartlogFlag {
    fn into_cc_type(self) -> Result<SmartlogFlag> {
        Ok(match self {
            GetSmartlogFlag::AddAllBookmarks => SmartlogFlag::AddAllBookmarks,
            GetSmartlogFlag::AddRemoteBookmarks => SmartlogFlag::AddRemoteBookmarks,
            GetSmartlogFlag::SkipPublicCommitsMetadata => SmartlogFlag::SkipPublicCommitsMetadata,
        })
    }
}

impl IntoCommitCloudType<CloudSmartlogFilter> for SmartlogFilter {
    fn into_cc_type(self) -> Result<CloudSmartlogFilter> {
        Ok(match self {
            SmartlogFilter::Timestamp(timestamp) => CloudSmartlogFilter::Timestamp(timestamp),
            SmartlogFilter::Version(version) => CloudSmartlogFilter::Version(version),
        })
    }
}

impl FromCommitCloudType<CloudReferencesData> for ReferencesData {
    fn from_cc_type(cc: CloudReferencesData) -> Result<Self> {
        Ok(ReferencesData {
            heads: cc.heads.map(map_cloud_into_id_ids),
            bookmarks: cc.bookmarks.map(|bms| {
                bms.into_iter()
                    .map(|(name, node)| (name, Id20::from_byte_array(node.0.into_byte_array())))
                    .collect()
            }),
            remote_bookmarks: cc.remote_bookmarks.map(rbs_from_cc_type).transpose()?,
            snapshots: cc.snapshots.map(map_cloud_into_id_ids),
            timestamp: cc.timestamp,
            version: cc.version,
            heads_dates: cc.heads_dates.map(|heads_dates| {
                heads_dates
                    .into_iter()
                    .map(|(idcsid, date)| (Id20::from_byte_array(idcsid.0.into_byte_array()), date))
                    .collect()
            }),
        })
    }
}

impl FromCommitCloudType<WorkspaceRemoteBookmark> for RemoteBookmark {
    fn from_cc_type(cc: WorkspaceRemoteBookmark) -> Result<RemoteBookmark> {
        Ok(RemoteBookmark {
            name: cc.name().clone(),
            remote: cc.remote().clone(),
            node: Some(cc.commit().0.into_byte_array().into()),
        })
    }
}

impl FromCommitCloudType<CloudSmartlogNode> for SmartlogNode {
    fn from_cc_type(cc: CloudSmartlogNode) -> Result<Self> {
        Ok(SmartlogNode {
            node: cc.node.0.into_byte_array().into(),
            phase: cc.phase,
            author: cc.author,
            date: cc.date,
            message: cc.message,
            parents: map_cloud_into_id_ids(cc.parents),
            bookmarks: cc.bookmarks,
            remote_bookmarks: cc.remote_bookmarks.map(rbs_from_cc_type).transpose()?,
        })
    }
}

impl FromCommitCloudType<CloudSmartlogData> for SmartlogData {
    fn from_cc_type(cc: CloudSmartlogData) -> Result<Self> {
        Ok(SmartlogData {
            nodes: cc
                .nodes
                .into_iter()
                .map(SmartlogNode::from_cc_type)
                .collect::<Result<Vec<SmartlogNode>>>()?,
            version: cc.version,
            timestamp: cc.timestamp,
        })
    }
}

impl FromCommitCloudType<CloudWorkspaceSharingData> for WorkspaceSharingData {
    fn from_cc_type(cc: CloudWorkspaceSharingData) -> Result<Self> {
        Ok(WorkspaceSharingData {
            acl_name: cc.acl_name,
            sharing_message: cc.sharing_message,
        })
    }
}

impl FromCommitCloudType<CloudHistoricalVersion> for HistoricalVersion {
    fn from_cc_type(cc: CloudHistoricalVersion) -> Result<Self> {
        Ok(HistoricalVersion {
            version_number: cc.version_number,
            timestamp: cc.timestamp,
        })
    }
}

impl FromCommitCloudType<CloudWorkspaceData> for WorkspaceData {
    fn from_cc_type(cc: CloudWorkspaceData) -> Result<Self> {
        Ok(WorkspaceData {
            name: cc.name,
            reponame: cc.reponame,
            version: cc.version,
            archived: cc.archived,
            timestamp: cc.timestamp,
        })
    }
}

fn map_id_into_cloud_ids(ids: Vec<Id20>) -> Result<Vec<CloudChangesetId>> {
    ids.into_iter()
        .map(|id| Ok(CloudChangesetId(Sha1::from_bytes(id)?)))
        .collect()
}

fn map_cloud_into_id_ids(c_ids: Vec<CloudChangesetId>) -> Vec<Id20> {
    c_ids
        .into_iter()
        .map(|c_id| Id20::from_byte_array(c_id.0.into_byte_array()))
        .collect::<Vec<Id20>>()
}

fn rbs_into_cc_type(rbs: Vec<RemoteBookmark>) -> Result<Vec<WorkspaceRemoteBookmark>> {
    rbs.into_iter().map(|rb| rb.into_cc_type()).collect()
}

fn rbs_from_cc_type(fbs: Vec<WorkspaceRemoteBookmark>) -> Result<Vec<RemoteBookmark>> {
    fbs.into_iter().map(RemoteBookmark::from_cc_type).collect()
}

pub(crate) fn strip_git_suffix(reponame: &str) -> &str {
    match reponame.strip_suffix(".git") {
        Some(reponame) => reponame,
        None => reponame,
    }
}
