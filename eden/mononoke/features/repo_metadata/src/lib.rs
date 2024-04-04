/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

#[cfg(fbcode_build)]
mod log;
mod process;
mod types;

use anyhow::Result;
use bookmarks::BookmarksRef;
use context::CoreContext;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use metaconfig_types::RepoConfigRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;

use crate::process::process_bookmark;
use crate::types::MetadataItem;

pub trait Repo = RepoConfigRef
    + RepoIdentityRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + BookmarksRef
    + Send
    + Sync;

/// Returns a stream of metadata about all files and directories in the repo.
pub fn repo_metadata<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
) -> impl Stream<Item = Result<MetadataItem>> + 'a {
    let bookmarks_to_log = &repo.repo_config().metadata_logger_config.bookmarks;
    stream::iter(bookmarks_to_log)
        .map(|bookmark| process_bookmark(ctx, repo, bookmark))
        .buffered(100)
        .try_flatten()
}
