/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Backtrace utilities for Sapling use-cases.
//!
//! ## Goals
//!
//! 1. Support hybrid (Python + native) backtraces (at least on some platforms).
//! 2. Support periodical backtrace for profiling. i.e. backtrace can be
//!    captured in a signal handler, is async-signal-safe.
//!
//! ## Design
//!
//! - Backtrace is captured in 2 steps.
//!   - Step 1: Collect stack frames (and Python code objects).
//!     This step should be async-signal-safe, and must pause the
//!     thread to be captured.
//!   - Step 2: Resolve stack frames (and Python code objects)
//!     to human-readable names. This step is not async-signal-safe,
//!     can run in a separate thread, and does not require the
//!     captured thread to be paused, although the Python interpreter
//!     should be alive to be able to extract strings from the the
//!     code objects.
//! - Python support is optional. This crate does not directly depend on Python
//!   crates. The Python support is a separate crate.

use std::ffi::c_void;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;

#[cfg(target_os = "linux")]
pub use unwind;
pub use unwind::Cursor;
pub use unwind::RegNum;

/// Place holder unwind for non-Linux systems.
#[cfg(not(target_os = "linux"))]
pub mod unwind {
    #[derive(Clone)]
    pub struct Cursor<'a>(std::marker::PhantomData<&'a ()>);
    pub enum RegNum {
        IP = 1,
        SP = 2,
    }
    impl<'a> Cursor<'a> {
        pub fn step(&mut self) -> Result<bool, ()> {
            Ok(false)
        }
        pub fn register(&self, _reg: RegNum) -> Result<usize, ()> {
            Err(())
        }
    }
}

/// Extend the default frame resolver to support resolving non-native
/// frames. For example, to extract Python frames.
pub trait SupplementalFrameResolver: Send + Sync + 'static {
    /// Extract [`SupplementalInfo`] from a frame.
    /// The current thread (with the frame) is paused.
    /// Must be async-signal-safe.
    fn maybe_extract_supplemental_info(&self, ip: usize, sp: usize) -> FrameDecision;

    /// Resolve a [`SupplementalInfo`] previously reported by `maybe_replace`.
    /// Can be non-async-signal-safe. The thread is not paused.
    fn resolve_supplemental_info(&self, info: &SupplementalInfo) -> Option<String>;
}

/// Return value of `extract_supplemental_info`.
#[derive(Clone, Copy, Debug)]
pub enum FrameDecision {
    /// Keep the native frame unchanged.
    Keep,
    /// Skip the native frame.
    /// For example, the Python frame resolver might want to skip all libpython
    /// frames to reduce noise.
    Skip,
    /// Replace the frame with customized info.
    Replace(SupplementalInfo),
}

/// Opaque data extracted from a frame for later resolution.
/// The size of this struct is designed to meet actual use-cases.
/// For now, Python frames can be represented as `[PyCodeObject*, line_number]`.
pub type SupplementalInfo = [usize; 2];

/// The [`SupplementalFrameResolver`] used by this process.
static SUPPLEMENTAL_FRAME_RESOLVER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Set the [`SupplementalFrameResolver`] used by this process.
pub fn set_supplemental_frame_resolver(resolver: &'static &'static dyn SupplementalFrameResolver) {
    SUPPLEMENTAL_FRAME_RESOLVER.store(
        resolver as *const &dyn SupplementalFrameResolver as *const () as *mut (),
        Ordering::Release,
    );
}

fn get_supplemental_frame_resolver() -> Option<&'static &'static dyn SupplementalFrameResolver> {
    let ptr = SUPPLEMENTAL_FRAME_RESOLVER.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // avoid dereference
        Some(unsafe { std::mem::transmute(ptr) })
    }
}

/// A captured stack frame.
/// This struct is designed to be "serialized" by memcpy.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Frame {
    pub ip: usize,
    pub sp: usize,
    /// Optional data extracted by `extract_supplemental_info`.
    pub info: Option<SupplementalInfo>,
}

impl Frame {
    /// Resolve this frame to a human-readable name.
    /// Uses the custom resolver if provided, otherwise falls back to default symbolization.
    pub fn resolve(&self) -> String {
        if let (Some(resolver), Some(data)) = (get_supplemental_frame_resolver(), &self.info) {
            if let Some(name) = resolver.resolve_supplemental_info(data) {
                return name;
            }
        }
        self.default_resolve()
    }

    fn default_resolve(&self) -> String {
        let mut resolved = None;
        // NOTE: `backtrace::resolve` might call its callback multiple times (e.g. inlined
        // functions). For simplicity, we assume callback is only once and use the last `symbol`.
        backtrace::resolve(self.ip as *mut c_void, |symbol| {
            if let Some(name) = symbol.name() {
                resolved = Some(name.to_string());
            }
        });
        match resolved {
            Some(s) => s,
            None => format!("{:#x}", self.ip),
        }
    }
}

/// Iterator over frames in a stack trace.
pub struct Backtrace<'a> {
    cursor: Cursor<'a>,
    ended: bool,
}

// This is defined as a macro, not a function intentionally.
// Shall this be a function, calling from the signal handler might
// use the "altstack" and lose the original stack information.
//
// PERF: `man libunwind` suggests defining `UNW_LOCAL_ONLY` for better
// performance if remote unwind is not needed. However the Rust `unwind`
// binding does not do it. Consider bypassing the binding to maximize
// performance.
#[cfg(target_os = "linux")]
#[macro_export]
macro_rules! try_backtrace {
    () => {{
        $crate::unwind::get_context!(context);
        $crate::unwind::Cursor::local(context).map($crate::Backtrace::new)
    }};
}

#[cfg(not(target_os = "linux"))]
#[macro_export]
macro_rules! try_backtrace {
    () => {{ None }};
}

impl<'a> Backtrace<'a> {
    /// Create a new frame iterator for the libunwind cursor.
    pub fn new(cursor: Cursor<'a>) -> Self {
        let ended = false;
        Self { cursor, ended }
    }
}

impl<'a> Iterator for Backtrace<'a> {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        loop {
            let ip = self.cursor.register(RegNum::IP).unwrap_or_default() as usize;
            let sp = self.cursor.register(RegNum::SP).unwrap_or_default() as usize;
            let decision = match get_supplemental_frame_resolver() {
                Some(resolver) => resolver.maybe_extract_supplemental_info(ip, sp),
                None => FrameDecision::Keep,
            };

            let (info, skip) = match decision {
                FrameDecision::Keep => (None, false),
                FrameDecision::Skip => (None, true),
                FrameDecision::Replace(info) => (Some(info), false),
            };

            let frame = Frame { ip, sp, info };

            self.ended = match self.cursor.step() {
                Err(_) | Ok(false) => true,
                Ok(true) => false,
            };

            if !skip {
                return Some(frame);
            }
        }
    }
}
