/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::convert::{TryFrom, TryInto};

use crate::errors::ErrorKind;
use anyhow::Error;
use apiserver_thrift::MononokeRevision::UnknownField;
use serde_derive::Serialize;

use apiserver_thrift::types::{
    MononokeGetBlobParams, MononokeGetBranchesParams, MononokeGetChangesetParams,
    MononokeGetFileHistoryParams, MononokeGetLastCommitOnPathParams, MononokeGetRawParams,
    MononokeGetTreeParams, MononokeIsAncestorParams, MononokeListDirectoryParams,
    MononokeListDirectoryUnodesParams, MononokeRevision,
};
use types::api::{DataRequest, HistoryRequest, TreeRequest};

#[derive(Debug, Clone, Serialize)]
pub enum Revision {
    CommitHash(String),
    Bookmark(String),
}

#[derive(Debug, Serialize)]
#[serde(tag = "method", content = "params")]
#[serde(rename_all = "snake_case")]
pub enum MononokeRepoQuery {
    GetRawFile {
        path: String,
        revision: Revision,
    },
    ListDirectory {
        path: String,
        revision: Revision,
    },
    ListDirectoryUnodes {
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
    GetFileHistory {
        path: String,
        revision: Revision,
        limit: i32,
        skip: i32,
    },
    GetLastCommitOnPath {
        path: String,
        revision: Revision,
    },
    IsAncestor {
        ancestor: Revision,
        descendant: Revision,
    },
    EdenGetData {
        request: DataRequest,
        stream: bool,
    },
    EdenGetHistory {
        request: HistoryRequest,
        stream: bool,
    },
    EdenGetTrees {
        request: DataRequest,
        stream: bool,
    },
    EdenPrefetchTrees {
        request: TreeRequest,
        stream: bool,
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

impl TryFrom<MononokeGetFileHistoryParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetFileHistoryParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo;
        let path = String::from_utf8(params.path)?;
        let limit = params.limit;
        let skip = params.skip;
        params.revision.try_into().map(|rev| MononokeQuery {
            repo,
            kind: MononokeRepoQuery::GetFileHistory {
                path,
                revision: rev,
                limit,
                skip,
            },
        })
    }
}

impl TryFrom<MononokeGetLastCommitOnPathParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetLastCommitOnPathParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo;
        let path = String::from_utf8(params.path)?;
        params.revision.try_into().map(|rev| MononokeQuery {
            repo,
            kind: MononokeRepoQuery::GetLastCommitOnPath {
                path,
                revision: rev,
            },
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

impl TryFrom<MononokeListDirectoryUnodesParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeListDirectoryUnodesParams) -> Result<MononokeQuery, Self::Error> {
        let repo = params.repo;
        let path = String::from_utf8(params.path)?;
        params.revision.try_into().map(|rev| MononokeQuery {
            repo,
            kind: MononokeRepoQuery::ListDirectoryUnodes {
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
