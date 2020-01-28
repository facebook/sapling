/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use serde::Deserialize;

#[derive(Deserialize)]
pub struct RequestLineInts {
    pub time: u64,
    pub responselen: Option<u64>,
    pub duration: u64,
}

#[derive(Deserialize)]
pub struct RequestLineNormals<'a> {
    pub command: &'a str,
    pub args: Option<String>,
    pub remote_args: Option<&'a str>,
    pub reponame: &'a str,
    pub user: Option<&'a str>,
    pub client_fullcommand: Option<&'a str>,
    pub client_hostname: Option<&'a str>,
    pub host: Option<&'a str>,
}

#[derive(Deserialize)]
pub struct RequestLine<'a> {
    pub int: RequestLineInts,
    #[serde(borrow)]
    pub normal: RequestLineNormals<'a>,
}
