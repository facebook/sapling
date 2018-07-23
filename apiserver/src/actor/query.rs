// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix::{Message, Syn};
use actix::dev::Request;
use failure::Error;
use futures_ext::BoxFuture;

use super::{MononokeRepoActor, MononokeRepoResponse};

#[derive(Debug)]
pub enum MononokeRepoQuery {
    GetRawFile {
        path: String,
        changeset: String,
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
    type Result = Result<Request<Syn, MononokeRepoActor, MononokeRepoQuery>, Error>;
}
