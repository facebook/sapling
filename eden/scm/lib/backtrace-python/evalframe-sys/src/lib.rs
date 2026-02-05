/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Raw FFI bindings for the evalframe C library.
//!
//! This crate provides low-level bindings to the Python frame evaluation
//! interception code in `evalframe.c`. The C code uses PEP 523 to insert
//! a pass-through function in the native stack to match Python stacks.

#![allow(non_camel_case_types)]

/// Enable or disable the pass-through eval frame function.
///
/// When enabled, Python frame evaluation goes through `Sapling_PyEvalFrame`
/// which keeps the frame state in its stack frame for native debuggers.
///
/// Note: calling this function when the Python interpreter is not initialized
/// is a no-op.
///
/// # Safety
/// This function is safe to call at any time, but should only be called
/// after Python initialization for the setting to take effect.
pub unsafe fn set_pass_through(enabled: bool) {
    sapling_cext_evalframe_set_pass_through(if enabled { 1 } else { 0 });
}

/// Check if frame resolution is supported on this Python version.
///
/// Returns non-zero if `resolve_frame` is expected to work.
pub fn resolve_frame_is_supported() -> bool {
    unsafe { sapling_cext_evalframe_resolve_frame_is_supported() != 0 }
}

/// Resolve a PyFrame pointer to a descriptive string.
///
/// Intended to be called by debuggers. Not thread-safe.
///
/// # Safety
/// The `frame_ptr` must be a valid `PyFrame*` or 0.
pub unsafe fn resolve_frame(frame_ptr: usize) -> *const u8 {
    unsafe { sapling_cext_evalframe_resolve_frame(frame_ptr) }
}

/// Extract code object and line number from a PyFrame.
///
/// # Safety
/// - `frame` must be a valid `PyFrame*` or null.
/// - `pline_no` must be a valid pointer to an `i32`.
/// - This function may be called without the GIL but the Python thread
///   that owns the frame must be paused.
pub unsafe fn extract_code_lineno_from_frame(
    frame: *mut libc::c_void,
    pline_no: *mut libc::c_int,
) -> *mut libc::c_void {
    unsafe { sapling_cext_evalframe_extract_code_lineno_from_frame(frame, pline_no) }
}

/// Resolve a code object to function name and filename.
///
/// # Safety
/// - `code` must be a valid `PyCodeObject*` or null.
/// - `pfilename` must be a valid pointer to receive the filename.
pub unsafe fn resolve_code_object(
    code: *mut libc::c_void,
    pfilename: *mut *const libc::c_char,
) -> *const libc::c_char {
    unsafe { sapling_cext_evalframe_resolve_code_object(code, pfilename) }
}

/// Get the address of the `Sapling_PyEvalFrame` function.
///
/// This is used to identify Python frames in native stack traces by
/// comparing instruction pointers against known offsets from this address.
pub fn sapling_py_eval_frame_addr() -> usize {
    Sapling_PyEvalFrame as *const () as usize
}

// Raw FFI declarations for evalframe.c
unsafe extern "C" {
    fn sapling_cext_evalframe_set_pass_through(enabled: u8);

    fn sapling_cext_evalframe_resolve_code_object(
        code: *mut libc::c_void,
        pfilename: *mut *const libc::c_char,
    ) -> *const libc::c_char;

    fn sapling_cext_evalframe_extract_code_lineno_from_frame(
        frame: *mut libc::c_void,
        pline_no: *mut libc::c_int,
    ) -> *mut libc::c_void;

    fn sapling_cext_evalframe_resolve_frame_is_supported() -> libc::c_int;

    fn sapling_cext_evalframe_resolve_frame(frame_ptr: usize) -> *const u8;

    // The pass-through eval frame function. We only need its address.
    fn Sapling_PyEvalFrame(tstate: usize, f: usize, exc: libc::c_int);
}
