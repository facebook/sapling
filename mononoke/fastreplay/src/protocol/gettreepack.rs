/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use hgproto::GettreepackArgs;
use mercurial_types::HgManifestId;
use serde::Deserialize;
use std::convert::TryInto;
use std::str::FromStr;

#[derive(Deserialize)]
pub struct RequestGettreepackArgs {
    rootdir: String,
    mfnodes: String,
    basemfnodes: String,
    directories: String,
    depth: Option<String>,
}

impl FromStr for RequestGettreepackArgs {
    type Err = Error;

    fn from_str(args: &str) -> Result<Self, Self::Err> {
        let ret = serde_json::from_str::<Vec<RequestGettreepackArgs>>(&args)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::msg(format!("Invalid RequestGettreepackArgs: {}", args)))?;
        Ok(ret)
    }
}

fn exclude_empty<'a>(e: &'a str) -> Option<&'a str> {
    if e.is_empty() {
        None
    } else {
        Some(e)
    }
}

impl TryInto<GettreepackArgs> for RequestGettreepackArgs {
    type Error = Error;

    fn try_into(self: Self) -> Result<GettreepackArgs, Self::Error> {
        let rootdir = self.rootdir.into_bytes().into();

        let mfnodes = self
            .mfnodes
            .split(" ")
            .filter_map(exclude_empty)
            .map(HgManifestId::from_str)
            .collect::<Result<_, _>>()?;

        let basemfnodes = self
            .basemfnodes
            .split(" ")
            .filter_map(exclude_empty)
            .map(HgManifestId::from_str)
            .collect::<Result<_, _>>()?;

        // See wireproto.escapearg in Mercurial
        let directories = self
            .directories
            .split(",")
            .filter_map(exclude_empty)
            .map(|_| Err(Error::msg("Gettreepack directories are not supported yet")))
            .collect::<Result<_, _>>()?;

        let depth = self.depth.map(|d| d.parse()).transpose()?;

        Ok(GettreepackArgs {
            rootdir,
            mfnodes,
            basemfnodes,
            directories,
            depth,
        })
    }
}
