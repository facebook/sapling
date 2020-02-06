/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use hgproto::GetbundleArgs;
use serde::Deserialize;
use std::str::FromStr;

use super::util::{extract_separated_list, split_separated_list};

pub struct RequestGetbundleArgs(pub GetbundleArgs);

#[derive(Deserialize)]
struct ReplayData<'a> {
    heads: &'a str,
    common: &'a str,
    bundlecaps: &'a str,
    listkeys: &'a str,
    phases: Option<&'a str>,
    #[allow(unused)]
    cg: Option<&'a str>,
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
            bundlecaps: split_separated_list(&json.bundlecaps, ",")
                .map(|e| e.as_bytes().clone().into())
                .collect(),
            listkeys: split_separated_list(&json.listkeys, ",")
                .map(|e| e.as_bytes().clone().into())
                .collect(),
            phases: json.phases.map(|p| p == "1").unwrap_or(false),
        };

        Ok(RequestGetbundleArgs(args))
    }
}
