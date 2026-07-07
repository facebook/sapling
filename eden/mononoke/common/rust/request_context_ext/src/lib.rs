/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Capture and restore the ambient `folly::RequestContext` around work that runs
//! on a separate thread (for example `spawn_blocking`), so that Artillery trace
//! context is preserved across the blocking-pool boundary.
//!
//! In `fbcode_build` this wraps `folly::RequestContext`. In OSS builds there is
//! no `folly::RequestContext`, so [`CapturedRequestContext`] is a no-op and
//! callers compile unchanged.

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use facebook::CapturedRequestContext;
#[cfg(fbcode_build)]
pub use facebook::install_request_context_hooks;
#[cfg(fbcode_build)]
pub use facebook::with_fresh_request_context;
#[cfg(not(fbcode_build))]
pub use oss::CapturedRequestContext;
#[cfg(not(fbcode_build))]
pub use oss::install_request_context_hooks;
#[cfg(not(fbcode_build))]
pub use oss::with_fresh_request_context;
