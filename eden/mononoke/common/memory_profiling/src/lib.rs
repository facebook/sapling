/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Memory profiling utilities for Mononoke HTTP services.
//!
//! This library provides reusable functions for generating memory flamegraphs
//! from jemalloc profiling data. It handles ACL-based access control and
//! graceful error handling when profiling is not available.

mod handler;

pub use handler::check_acl_access;
pub use handler::check_profiling_available;
pub use handler::generate_flamegraph;
