/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(test, type_length_limit = "2097152")]
// Used to avoid too much copy-and-paste in hook_running.
// Tracking issue https://github.com/rust-lang/rust/issues/41517 suggests it's reasonably safe to usse
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

pub use hook_running::run_hooks;
pub use hooks::CrossRepoPushSource;
pub use processing::run_post_resolve_action;
pub use push_redirector::{PushRedirector, PushRedirectorArgs};
pub use resolver::{
    resolve, BundleResolverError, BundleResolverResultExt, Changesets, CommonHeads,
    InfiniteBookmarkPush, NonFastForwardPolicy, PlainBookmarkPush, PostResolveAction,
    PostResolveBookmarkOnlyPushRebase, PostResolveInfinitePush, PostResolvePush,
    PostResolvePushRebase, PushrebaseBookmarkSpec, UploadedBonsais, UploadedHgChangesetIds,
};
pub use response::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};
pub use upload_changesets::upload_changeset;
