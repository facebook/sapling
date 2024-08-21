/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarksRef;
use bookmarks_cache::BookmarksCacheRef;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use filestore::FilestoreConfigRef;
use git_symbolic_refs::GitSymbolicRefsRef;
use metaconfig_types::RepoConfigRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;

pub mod bookmarks_provider;
pub mod generator;
pub mod mapping;
pub mod pack_processor;
mod store;
pub mod types;
pub mod utils;

const HEAD_REF: &str = "HEAD";
const TAGS_PREFIX: &str = "tags/";
const REF_PREFIX: &str = "refs/";
const PACKFILE_SUFFIX: &str = ".pack";

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + RepoDerivedDataArc
    + BookmarksRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + FilestoreConfigRef
    + RepoDerivedDataRef
    + GitSymbolicRefsRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + BookmarksCacheRef
    + RepoConfigRef
    + Send
    + Sync;
