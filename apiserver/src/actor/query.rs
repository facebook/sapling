// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::{TryFrom, TryInto};

use apiserver_thrift::MononokeRevision::UnknownField;
use bytes::Bytes;
use errors::ErrorKind;
use failure::Error;
use http::uri::Uri;

use apiserver_thrift::types::{
    MononokeGetBranchesParams, MononokeGetChangesetParams, MononokeGetRawParams,
    MononokeListDirectoryParams, MononokeRevision,
};

use super::lfs::BatchRequest;

#[derive(Debug)]
pub enum Revision {
    CommitHash(String),
    Bookmark(String),
}

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetRawFile {
        path: String,
        revision: Revision,
    },
    GetHgFile {
        filenode: String,
    },
    ListDirectory {
        path: String,
        revision: Revision,
    },
    GetBlobContent {
        hash: String,
    },
    GetTree {
        hash: String,
    },
    GetChangeset {
        revision: Revision,
    },
    GetBranches,
    IsAncestor {
        proposed_ancestor: String,
        proposed_descendent: String,
    },
    DownloadLargeFile {
        oid: String,
    },
    LfsBatch {
        repo_name: String,
        req: BatchRequest,
        lfs_url: Option<Uri>,
    },
    UploadLargeFile {
        oid: String,
        body: Bytes,
    },
}

pub struct MononokeQuery {
    pub kind: MononokeRepoQuery,
    pub repo: String,
}

impl TryFrom<MononokeGetRawParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetRawParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo;
        let path = String::from_utf8(params.path)?;
        params.revision.try_into().map(|rev| MononokeQuery {
            repo,
            kind: MononokeRepoQuery::GetRawFile {
                path,
                revision: rev,
            },
        })
    }
}

impl TryFrom<MononokeGetChangesetParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetChangesetParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo;
        params.revision.try_into().map(|rev| MononokeQuery {
            repo,
            kind: MononokeRepoQuery::GetChangeset { revision: rev },
        })
    }
}

impl TryFrom<MononokeGetBranchesParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetBranchesParams) -> Result<MononokeQuery, Self::Error> {
        Ok(MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetBranches,
        })
    }
}

impl TryFrom<MononokeListDirectoryParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeListDirectoryParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo;
        let path = String::from_utf8(params.path)?;
        params.revision.try_into().map(|rev| MononokeQuery {
            repo,
            kind: MononokeRepoQuery::ListDirectory {
                path,
                revision: rev,
            },
        })
    }
}

impl TryFrom<MononokeRevision> for Revision {
    type Error = Error;

    fn try_from(rev: MononokeRevision) -> Result<Revision, Error> {
        match rev {
            MononokeRevision::commit_hash(hash) => Ok(Revision::CommitHash(hash)),
            MononokeRevision::bookmark(bookmark) => Ok(Revision::Bookmark(bookmark)),
            UnknownField(_) => Err(ErrorKind::InvalidInput(
                format!("Invalid MononokeRevision {:?}", rev),
                None,
            )
            .into()),
        }
    }
}
