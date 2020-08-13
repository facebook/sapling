/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::NoOpts;
use super::Repo;
use super::Result;
use super::IO;
use edenapi::EdenApiBlocking;

pub fn run(_opts: NoOpts, io: &mut IO, repo: Repo) -> Result<u8> {
    let client = edenapi::Builder::from_config(repo.config())?.build()?;
    let meta = client.health_blocking()?;
    io.write(format!("{:#?}\n", &meta))?;
    Ok(0)
}

pub fn name() -> &'static str {
    "debughttp"
}

pub fn doc() -> &'static str {
    "check whether the EdenAPI server is reachable"
}
