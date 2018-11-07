// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryFrom;

use bytes::Bytes;
use failure::Error;

use http::uri::Uri;

use apiserver_thrift::types::MononokeGetRawParams;

use super::lfs::BatchRequest;

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetRawFile {
        path: String,
        changeset: String,
    },
    ListDirectory {
        path: String,
        changeset: String,
    },
    GetBlobContent {
        hash: String,
    },
    GetTree {
        hash: String,
    },
    GetChangeset {
        hash: String,
    },
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
                changeset: params.changeset,
            },
        })
    }
}
