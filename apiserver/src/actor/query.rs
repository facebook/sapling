// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::{TryFrom, TryInto};

use crate::errors::ErrorKind;
use apiserver_thrift::MononokeRevision::UnknownField;
use bytes::Bytes;
use failure::Error;
use http::uri::Uri;

use apiserver_thrift::types::{
    MononokeGetBlobParams, MononokeGetBranchesParams, MononokeGetChangesetParams,
    MononokeGetRawParams, MononokeGetTreeParams, MononokeIsAncestorParams,
    MononokeListDirectoryParams, MononokeRevision,
};
use types::api::{DataRequest, HistoryRequest};

use super::lfs::BatchRequest;

#[derive(Debug, Clone)]
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
    GetFileHistory {
        filenode: String,
        path: String,
        depth: Option<u32>,
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
        ancestor: Revision,
        descendant: Revision,
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
    EdenGetData(DataRequest),
    EdenGetHistory(HistoryRequest),
    EdenGetTrees(DataRequest),
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

impl TryFrom<MononokeIsAncestorParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeIsAncestorParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo.clone();
        let descendant = params.descendant.clone();
        params.ancestor.try_into().and_then(move |ancestor| {
            descendant.try_into().map(|descendant| MononokeQuery {
                repo,
                kind: MononokeRepoQuery::IsAncestor {
                    ancestor,
                    descendant,
                },
            })
        })
    }
}

impl TryFrom<MononokeGetBlobParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetBlobParams) -> Result<MononokeQuery, Self::Error> {
        Ok(MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetBlobContent {
                hash: params.blob_hash.hash,
            },
        })
    }
}

impl TryFrom<MononokeGetTreeParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetTreeParams) -> Result<MononokeQuery, Self::Error> {
        Ok(MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetTree {
                hash: params.tree_hash.hash,
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
