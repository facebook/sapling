/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(async_closure)]

//! State for a single source control Repo

mod client;
mod errors;

pub use client::{gettreepack_entries, RepoClient, WireprotoLogging};
pub use mononoke_repo::{streaming_clone, MononokeRepo};
pub use repo_read_write_status::RepoReadWriteFetcher;
pub use unbundle::{RepoSyncTarget, CONFIGERATOR_PUSHREDIRECT_ENABLE};
