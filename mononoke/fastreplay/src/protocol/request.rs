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
pub struct RequestLineNormals {
    pub command: String,
    pub args: String,
    pub reponame: String,
    pub user: Option<String>,
    pub client_fullcommand: Option<String>,
    pub client_hostname: Option<String>,
    pub host: Option<String>,
}

#[allow(unused)]
#[derive(Deserialize)]
pub struct RequestLine {
    pub int: RequestLineInts,
    pub normal: RequestLineNormals,
}
