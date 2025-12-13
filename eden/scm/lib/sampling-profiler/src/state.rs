/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicPtr;

/// If non-zero, only profile the matching thread.
/// Used by the signal handler.
pub static FOCUS_THREAD_ID: AtomicI32 = AtomicI32::new(0);

/// Frame information to write to. Used by the signal handler.
pub static PIPE_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

/// Timer ID for the SIGPROF timer. Used to clean up the timer on stop().
pub static TIMER_ID: AtomicPtr<libc::c_void> = AtomicPtr::new(std::ptr::null_mut());
