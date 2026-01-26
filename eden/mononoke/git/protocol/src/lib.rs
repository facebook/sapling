/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::sync::Arc;

use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarksRef;
use bookmarks_cache::BookmarksCacheRef;
use buffered_weighted::WeightObserver;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use filestore::FilestoreConfigRef;
use git_ref_content_mapping::GitRefContentMappingRef;
use git_symbolic_refs::GitSymbolicRefsRef;
use git_types::CgdmChangesetDividerArc;
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
const REF_PREFIX: &str = "refs/";
const TAGS_PREFIX: &str = "tags/";
const HEADS_PREFIX: &str = "heads/";
const PACKFILE_SUFFIX: &str = ".pack";

/// Type alias for an optional weight observer for tracking buffer usage.
pub type OptionalWeightObserver = Option<Arc<dyn WeightObserver>>;

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + RepoDerivedDataArc
    + BookmarksRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + GitRefContentMappingRef
    + FilestoreConfigRef
    + RepoDerivedDataRef
    + GitSymbolicRefsRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + BookmarksCacheRef
    + RepoConfigRef
    + CgdmChangesetDividerArc
    + Send
    + Sync;
