/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(test, type_length_limit = "2097152")]
#![feature(trait_alias)]

mod changegroup;
mod errors;
mod hook_running;
mod processing;
mod push_redirector;
mod rate_limits;
mod resolver;
mod response;
mod stats;
mod upload_blobs;
mod upload_changesets;

use bonsai_hg_mapping::BonsaiHgMappingArc;
use bookmarks::BookmarksRef;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphWriterArc;
use filestore::FilestoreConfigRef;
pub use hook_running::run_hooks;
pub use hooks::CrossRepoPushSource;
use mercurial_mutation::HgMutationStoreArc;
use phases::PhasesRef;
pub use processing::run_post_resolve_action;
pub use push_redirector::PushRedirector;
pub use push_redirector::PushRedirectorArgs;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_identity::RepoIdentityRef;
pub use resolver::resolve;
pub use resolver::BundleResolverError;
pub use resolver::BundleResolverResultExt;
pub use resolver::Changesets;
pub use resolver::CommonHeads;
pub use resolver::InfiniteBookmarkPush;
pub use resolver::NonFastForwardPolicy;
pub use resolver::PlainBookmarkPush;
pub use resolver::PostResolveAction;
pub use resolver::PostResolveBookmarkOnlyPushRebase;
pub use resolver::PostResolveInfinitePush;
pub use resolver::PostResolvePush;
pub use resolver::PostResolvePushRebase;
pub use resolver::PushrebaseBookmarkSpec;
pub use resolver::UploadedBonsais;
pub use resolver::UploadedHgChangesetIds;
pub use response::UnbundleBookmarkOnlyPushRebaseResponse;
pub use response::UnbundleInfinitePushResponse;
pub use response::UnbundlePushRebaseResponse;
pub use response::UnbundlePushResponse;
pub use response::UnbundleResponse;
pub use upload_changesets::upload_changeset;

pub trait Repo = CommitGraphArc
    + CommitGraphWriterArc
    + BonsaiHgMappingArc
    + BookmarksRef
    + RepoDerivedDataArc
    + PhasesRef
    + HgMutationStoreArc
    + RepoBlobstoreRef
    + RepoBlobstoreArc
    + FilestoreConfigRef
    + RepoIdentityRef
    + remotefilelog::RepoLike
    + Clone
    + 'static
    + Send
    + Sync;
