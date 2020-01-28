/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize)]
pub struct RequestLineInts {
    pub time: u64,
    pub responselen: Option<u64>,
    pub duration: u64,
}

#[derive(Deserialize)]
pub struct RequestLineNormals<'a> {
    #[serde(borrow)]
    pub command: Cow<'a, str>,
    #[serde(borrow)]
    pub args: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub remote_args: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub reponame: Cow<'a, str>,
    #[serde(borrow)]
    pub user: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub client_fullcommand: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub client_hostname: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub host: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
pub struct RequestLine<'a> {
    pub int: RequestLineInts,
    #[serde(borrow)]
    pub normal: RequestLineNormals<'a>,
}
