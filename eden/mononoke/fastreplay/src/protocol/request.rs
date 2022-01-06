/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize)]
pub struct RequestLineInts {
    pub time: i64,
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
    #[serde(borrow)]
    pub mononoke_session_uuid: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
pub struct RequestLine<'a> {
    pub int: RequestLineInts,
    #[serde(borrow)]
    pub normal: RequestLineNormals<'a>,
}

impl<'a> RequestLine<'a> {
    pub fn duration_us(&self) -> u64 {
        if self.is_mononoke() {
            self.int.duration
        } else {
            // Mercurial logs response time in milliseconds
            self.int.duration * 1000
        }
    }

    pub fn server_type(&self) -> &'static str {
        if self.is_mononoke() {
            "mononoke"
        } else {
            "mercurial"
        }
    }

    pub fn is_mononoke(&self) -> bool {
        self.normal.mononoke_session_uuid.is_some()
    }
}
