/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![cfg_attr(test, type_length_limit = "2097152")]

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
pub use processing::run_post_resolve_action;
pub use push_redirector::{PushRedirector, CONFIGERATOR_PUSHREDIRECT_ENABLE};
pub use resolver::{
    resolve, BundleResolverError, Changesets, CommonHeads, InfiniteBookmarkPush,
    NonFastForwardPolicy, PlainBookmarkPush, PostResolveAction, PostResolveBookmarkOnlyPushRebase,
    PostResolveInfinitePush, PostResolvePush, PostResolvePushRebase, PushrebaseBookmarkSpec,
    UploadedBonsais, UploadedHgChangesetIds,
};
pub use response::{
    UnbundleBookmarkOnlyPushRebaseResponse, UnbundleInfinitePushResponse,
    UnbundlePushRebaseResponse, UnbundlePushResponse, UnbundleResponse,
};
