// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryFrom;

use bytes::Bytes;
use failure::Error;

use http::uri::Uri;

use apiserver_thrift::types::{
    MononokeGetBranchesParams, MononokeGetChangesetParams, MononokeGetRawParams,
    MononokeListDirectoryParams,
};

use super::lfs::BatchRequest;

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetRawFile {
        path: String,
        revision: String,
    },
    ListDirectory {
        path: String,
        revision: String,
    },
    GetBlobContent {
        hash: String,
    },
    GetTree {
        hash: String,
    },
    GetChangeset {
        revision: String,
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
        Ok(MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetRawFile {
                path: String::from_utf8(params.path)?,
                revision: params.changeset,
            },
        })
    }
}

impl TryFrom<MononokeGetChangesetParams> for MononokeQuery {
    type Error = Error;

    fn try_from(params: MononokeGetChangesetParams) -> Result<MononokeQuery, Self::Error> {
        Ok(MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetChangeset {
                revision: params.revision,
            },
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
        Ok(MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::ListDirectory {
                path: String::from_utf8(params.path)?,
                revision: params.revision,
            },
        })
    }
}
