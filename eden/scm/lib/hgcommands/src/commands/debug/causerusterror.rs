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
use taggederror::{intentional_error, AnyhowExt, Fault};

pub fn run(_opts: NoOpts, _io: &mut IO, _repo: Repo) -> Result<u8> {
    // Add additional metadata via AnyhowExt trait to an anyhow::Error or anyhow::Result
    Ok(intentional_error(false).with_fault(Fault::Request)?)
}

pub fn name() -> &'static str {
    "debugcauserusterror"
}

pub fn doc() -> &'static str {
    "cause an error to be generated in rust for testing error handling"
}
