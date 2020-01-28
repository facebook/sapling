/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

mod getbundle;
mod getpack;
mod gettreepack;
mod request;
mod util;

use anyhow::Error;
use std::str::FromStr;

use getbundle::RequestGetbundleArgs;
use getpack::RequestGetpackArgs;
use gettreepack::RequestGettreepackArgs;
use request::RequestLine;

pub enum Request {
    Gettreepack(RequestGettreepackArgs),
    GetpackV1(RequestGetpackArgs),
    GetpackV2(RequestGetpackArgs),
    Getbundle(RequestGetbundleArgs),
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
            "gettreepack" => Request::Gettreepack(req.normal.args.parse()?),
            "getbundle" => Request::Getbundle(req.normal.args.parse()?),
            "getpackv1" => Request::GetpackV1(req.normal.args.parse()?),
            "getpackv2" => Request::GetpackV2(req.normal.args.parse()?),
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
