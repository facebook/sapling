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
pub use push_redirector::PushRedirector;
pub use push_redirector::PushRedirectorArgs;
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
