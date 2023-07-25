/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use mononoke_types::hash::GitSha1;

use crate::thrift;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct MappedGitCommitId(GitSha1);

impl MappedGitCommitId {
    pub fn new(sha1: GitSha1) -> Self {
        Self(sha1)
    }

    pub fn oid(&self) -> &GitSha1 {
        &self.0
    }
}

impl TryFrom<thrift::MappedGitCommitId> for MappedGitCommitId {
    type Error = Error;

    fn try_from(t: thrift::MappedGitCommitId) -> Result<Self, Error> {
        let oid = GitSha1::from_bytes(&t.oid.0)?;

        Ok(Self(oid))
    }
}

impl From<MappedGitCommitId> for thrift::MappedGitCommitId {
    fn from(ch: MappedGitCommitId) -> thrift::MappedGitCommitId {
        thrift::MappedGitCommitId {
            oid: ch.0.into_thrift(),
        }
    }
}
