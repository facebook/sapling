// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![cfg_attr(test, type_length_limit = "2097152")]

mod changegroup;
pub mod errors;
mod getbundle_response;
mod resolver;
mod stats;
mod upload_blobs;
mod upload_changesets;

pub use getbundle_response::create_getbundle_response;
pub use resolver::{
    resolve, BookmarkPush, BundleResolverError, PostResolveAction,
    PostResolveBookmarkOnlyPushRebase, PostResolvePush, PostResolvePushRebase,
    PushrebaseBookmarkSpec,
};
