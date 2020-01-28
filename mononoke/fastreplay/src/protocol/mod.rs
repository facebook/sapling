/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

mod gettreepack;
mod request;

use anyhow::Error;
use hgproto::GettreepackArgs;
use std::convert::TryInto;
use std::str::FromStr;

use gettreepack::RequestGettreepackArgs;
use request::RequestLine;

pub enum Request {
    Gettreepack(GettreepackArgs),
}

pub struct RepoRequest {
    pub reponame: String,
    pub request: Request,
}

impl FromStr for RepoRequest {
    type Err = Error;

    fn from_str(req: &str) -> Result<Self, Self::Err> {
        let req: RequestLine = serde_json::from_str(&req)?;

        let request = match req.normal.command.as_ref() {
            "gettreepack" => {
                let args: RequestGettreepackArgs = req.normal.args.parse()?;
                Request::Gettreepack(args.try_into()?)
            }
            cmd @ _ => {
                return Err(Error::msg(format!("Command not supported: {}", cmd)));
            }
        };

        Ok(RepoRequest {
            reponame: req.normal.reponame,
            request,
        })
    }
}
