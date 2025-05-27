/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::CommitIdentityScheme;
pub mod changeset;
pub mod error;
pub mod references;
pub mod smartlog;

pub use error::CommitCloudError;
pub use error::CommitCloudInternalError;
pub use error::CommitCloudUserError;
pub use references::ClientInfo;
pub use references::LocalBookmarksMap;
pub use references::ReferencesData;
pub use references::RemoteBookmarksMap;
pub use references::UpdateReferencesParams;
pub use references::WorkspaceCheckoutLocation;
pub use references::WorkspaceHead;
pub use references::WorkspaceLocalBookmark;
pub use references::WorkspaceRemoteBookmark;
pub use references::WorkspaceSnapshot;
pub use smartlog::SmartlogData;
pub use smartlog::SmartlogFilter;
pub use smartlog::SmartlogFlag;
pub use smartlog::SmartlogNode;

#[derive(Debug, Clone)]
pub enum ChangesetScheme {
    Hg,
    Git,
}

impl TryFrom<CommitIdentityScheme> for ChangesetScheme {
    type Error = anyhow::Error;
    fn try_from(scheme: CommitIdentityScheme) -> Result<Self, Self::Error> {
        let res = match scheme {
            CommitIdentityScheme::HG => ChangesetScheme::Hg,
            CommitIdentityScheme::GIT => ChangesetScheme::Git,
            _ => anyhow::bail!("commit cloud: repo has unsupported scheme: {:?}", scheme),
        };
        Ok(res)
    }
}

pub struct WorkspaceSharingData {
    pub acl_name: String,
    pub sharing_message: String,
}

pub struct HistoricalVersion {
    pub version_number: i64,
    pub timestamp: i64,
}

pub struct WorkspaceData {
    pub name: String,
    pub reponame: String,
    pub version: u64,
    pub archived: bool,
    pub timestamp: i64,
}
