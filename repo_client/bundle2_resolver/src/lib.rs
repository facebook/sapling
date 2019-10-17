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
pub mod errors;
mod resolver;
mod stats;
mod upload_blobs;
mod upload_changesets;

pub use resolver::{
    resolve, BundleResolverError, Changesets, CommonHeads, InfiniteBookmarkPush, PlainBookmarkPush,
    PostResolveAction, PostResolveBookmarkOnlyPushRebase, PostResolveInfinitePush, PostResolvePush,
    PostResolvePushRebase, PushrebaseBookmarkSpec,
};
