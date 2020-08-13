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

pub fn run(_opts: NoOpts, _io: &mut IO, repo: Repo) -> Result<u8> {
    let store_path = repo.store_path();
    let patterns = [
        "00changelog.*",
        "hgcommits/**/*",
        "metalog/**/*",
        "mutation/**/*",
    ];
    fsyncglob::fsync_glob(store_path, &patterns, None);
    Ok(0)
}

pub fn name() -> &'static str {
    "debugfsync"
}

pub fn doc() -> &'static str {
    "call fsync on newly modified key storage files"
}
