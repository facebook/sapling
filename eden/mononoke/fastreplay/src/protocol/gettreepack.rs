/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use hgproto::GettreepackArgs;
use mononoke_types::MPath;
use serde::Deserialize;
use std::borrow::Cow;
use std::str::FromStr;

use super::util::{extract_separated_list, split_separated_list};

pub struct RequestGettreepackArgs(pub GettreepackArgs);

#[derive(Deserialize)]
pub struct ReplayData<'a> {
    rootdir: Cow<'a, str>,
    mfnodes: Cow<'a, str>,
    basemfnodes: Cow<'a, str>,
    directories: Cow<'a, str>,
    depth: Option<Cow<'a, str>>,
}

impl FromStr for RequestGettreepackArgs {
    type Err = Error;

    fn from_str(args: &str) -> Result<Self, Self::Err> {
        let json = serde_json::from_str::<Vec<ReplayData>>(&args)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::msg(format!("Invalid Gettreepack ReplayData: {}", args)))?;

        // See wireproto.escapearg in Mercurial
        let directories = split_separated_list(json.directories.as_ref(), ",")
            .map(|_| Err(Error::msg("Gettreepack directories are not supported yet")))
            .collect::<Result<_, _>>()?;

        let args = GettreepackArgs {
            rootdir: MPath::new_opt(&*json.rootdir)?,
            mfnodes: extract_separated_list(json.mfnodes.as_ref(), " ")?,
            basemfnodes: extract_separated_list(json.basemfnodes.as_ref(), " ")?,
            directories,
            depth: json.depth.map(|d| d.parse()).transpose()?,
        };

        Ok(RequestGettreepackArgs(args))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() -> Result<(), Error> {
        RequestGettreepackArgs::from_str(include_str!("./fixtures/gettreepack.json"))?;
        Ok(())
    }
}
