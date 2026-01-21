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

mod backtrace_collector;
#[cfg(target_os = "linux")]
mod frame_handler;
#[cfg(target_os = "linux")]
mod osutil;
#[cfg_attr(not(target_os = "linux"), path = "profiler_dummy.rs")]
mod profiler;
#[cfg(target_os = "linux")]
mod signal_handler;

pub use backtrace_collector::BacktraceCollector;
pub use backtrace_ext; // re-export
pub use libc;
pub use profiler::Profiler;

/// Function to process backtraces.
pub type ResolvedBacktraceProcessFunc = Box<dyn FnMut(&[String]) + Send + Sync + 'static>;
