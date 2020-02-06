/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use hgproto::GettreepackArgs;
use serde::Deserialize;
use std::str::FromStr;

use super::util::{extract_separated_list, split_separated_list};

pub struct RequestGettreepackArgs(pub GettreepackArgs);

#[derive(Deserialize)]
pub struct ReplayData<'a> {
    rootdir: &'a str,
    mfnodes: &'a str,
    basemfnodes: &'a str,
    directories: &'a str,
    depth: Option<&'a str>,
}

impl FromStr for RequestGettreepackArgs {
    type Err = Error;

    fn from_str(args: &str) -> Result<Self, Self::Err> {
        let json = serde_json::from_str::<Vec<ReplayData>>(&args)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::msg(format!("Invalid Gettreepack ReplayData: {}", args)))?;

        // See wireproto.escapearg in Mercurial
        let directories = split_separated_list(json.directories, ",")
            .map(|_| Err(Error::msg("Gettreepack directories are not supported yet")))
            .collect::<Result<_, _>>()?;

        let args = GettreepackArgs {
            rootdir: json.rootdir.as_bytes().clone().into(),
            mfnodes: extract_separated_list(json.mfnodes, " ")?,
            basemfnodes: extract_separated_list(json.basemfnodes, " ")?,
            directories,
            depth: json.depth.map(|d| d.parse()).transpose()?,
        };

        Ok(RequestGettreepackArgs(args))
    }
}
