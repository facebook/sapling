/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Sampling profiler for Sapling.
//!
//! - Sample the main thread periodically (ex. every second)
//! - Resolve Python frames (by backtrace-python)
//!
//! Currently implemented for Linux.

mod frame_handler;
mod osutil;
mod profiler;

pub use backtrace_ext; // re-export
pub use profiler::Profiler;
