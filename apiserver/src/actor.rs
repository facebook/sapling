// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use actix::{Actor, Context, Handler, Message};
use failure::Result;

#[derive(Debug)]
pub enum MononokeQuery {
    GetBlobContent { repo: String, hash: String },
}

impl Message for MononokeQuery {
    type Result = Result<String>;
}

pub struct MononokeActor;

impl Actor for MononokeActor {
    type Context = Context<Self>;
}

impl Handler<MononokeQuery> for MononokeActor {
    type Result = Result<String>;

    fn handle(&mut self, msg: MononokeQuery, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            MononokeQuery::GetBlobContent { repo, hash } => {
                Ok(format!("got repo: {} hash: {}", repo, hash))
            }
        }
    }
}
