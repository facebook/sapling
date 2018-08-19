// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryFrom;

use actix::Message;
use actix::dev::Request;
use failure::Error;
use futures_ext::BoxFuture;

use apiserver_thrift::types::MononokeGetRawParams;

use super::{MononokeRepoActor, MononokeRepoResponse};

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
}

impl Message for MononokeRepoQuery {
    type Result = Result<BoxFuture<MononokeRepoResponse, Error>, Error>;
}

pub struct MononokeQuery {
    pub kind: MononokeRepoQuery,
    pub repo: String,
}

impl Message for MononokeQuery {
    type Result = Result<Request<MononokeRepoActor, MononokeRepoQuery>, Error>;
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
