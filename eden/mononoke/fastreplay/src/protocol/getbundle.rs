/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use hgproto::GetbundleArgs;
use serde::Deserialize;
use std::borrow::Cow;
use std::str::FromStr;

use super::util::{extract_separated_list, split_separated_list};

pub struct RequestGetbundleArgs(pub GetbundleArgs);

#[derive(Deserialize)]
struct ReplayData<'a> {
    heads: Cow<'a, str>,
    common: Cow<'a, str>,
    bundlecaps: Cow<'a, str>,
    listkeys: Cow<'a, str>,
    phases: Option<Cow<'a, str>>,
    #[allow(unused)]
    cg: Option<Cow<'a, str>>,
}

impl FromStr for RequestGetbundleArgs {
    type Err = Error;

    fn from_str(args: &str) -> Result<Self, Self::Err> {
        let json = serde_json::from_str::<Vec<ReplayData>>(&args)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::msg(format!("Invalid Getbundle ReplayData: {}", args)))?;

        let args = GetbundleArgs {
            heads: extract_separated_list(&json.heads, " ")?,
            common: extract_separated_list(&json.common, " ")?,
            bundlecaps: split_separated_list(json.bundlecaps.as_ref(), ",")
                .map(|e| e.as_bytes().clone().into())
                .collect(),
            listkeys: split_separated_list(json.listkeys.as_ref(), ",")
                .map(|e| e.as_bytes().clone().into())
                .collect(),
            phases: json.phases.map(|p| p == "1").unwrap_or(false),
        };

        Ok(RequestGetbundleArgs(args))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() -> Result<(), Error> {
        RequestGetbundleArgs::from_str(include_str!("./fixtures/getbundle.json"))?;
        Ok(())
    }
}
