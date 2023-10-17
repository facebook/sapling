/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarksRef;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream::BoxStream;
use git_symbolic_refs::GitSymbolicRefs;
use packfile::types::PackfileItem;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;

use crate::types::PackInputStreamRequest;

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + BookmarksRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + RepoDerivedDataRef
    + GitSymbolicRefs
    + CommitGraphRef
    + Send
    + Sync;

pub fn generate_pack_item_stream<'a>(
    _ctx: &'a CoreContext,
    _repo: &'a impl Repo,
    _request: PackInputStreamRequest,
) -> (BoxStream<'a, Result<PackfileItem>>, usize) {
    todo!()
}
