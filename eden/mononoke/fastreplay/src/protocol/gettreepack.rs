/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use hgproto::{batch, GettreepackArgs};
use mononoke_types::MPath;
use serde::Deserialize;
use std::borrow::Cow;
use std::str::FromStr;

use super::util::extract_separated_list;

pub struct RequestGettreepackArgs(pub GettreepackArgs);

#[derive(Deserialize)]
pub struct ReplayData<'a> {
    rootdir: Cow<'a, str>,
    mfnodes: Cow<'a, str>,
    basemfnodes: Cow<'a, str>,
    directories: Cow<'a, str>,
    depth: Option<Cow<'a, str>>,
}

fn parse_directories(dirs: &str) -> Result<Vec<Vec<u8>>, Error> {
    // We do this early check, because later we'll pop off the last element and check that it's
    // empty, so this has to go here.
    if dirs.is_empty() {
        return Ok(vec![]);
    }

    // Directories have a trailing comma, and are wireproto encoded. So, we decode each entry, and
    // skip the last one (which we verify is empty).
    let mut ret = dirs
        .split(',')
        .map(|d| batch::unescape(d.as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;

    match ret.pop() {
        Some(v) if v.is_empty() => {
            // Noop
        }
        e => {
            return Err(format_err!(
                "Invalid trailing element in parse_directories: {:?}",
                e
            ));
        }
    };

    Ok(ret)
}

impl FromStr for RequestGettreepackArgs {
    type Err = Error;

    fn from_str(args: &str) -> Result<Self, Self::Err> {
        let json = serde_json::from_str::<Vec<ReplayData>>(&args)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::msg(format!("Invalid Gettreepack ReplayData: {}", args)))?;

        let args = GettreepackArgs {
            rootdir: MPath::new_opt(&*json.rootdir)?,
            mfnodes: extract_separated_list(json.mfnodes.as_ref(), " ")?,
            basemfnodes: extract_separated_list(json.basemfnodes.as_ref(), " ")?,
            directories: parse_directories(json.directories.as_ref())?
                .into_iter()
                .map(|d| d.into())
                .collect(),
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

    #[test]
    fn test_parse_directories() -> Result<(), Error> {
        assert_eq!(parse_directories("foo,")?, vec![b"foo".to_vec()]);

        assert_eq!(parse_directories("foo:obar,")?, vec![b"foo,bar".to_vec()]);

        assert_eq!(
            parse_directories("foo,bar,")?,
            vec![b"foo".to_vec(), b"bar".to_vec()]
        );

        assert_eq!(parse_directories(",")?, vec![b"".to_vec()]);

        assert_eq!(parse_directories("")?, Vec::<Vec<u8>>::new());

        assert!(parse_directories(",foo").is_err());

        Ok(())
    }
}
