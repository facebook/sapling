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

/// Evalframe mode for `set_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EvalFrameMode {
    /// Disabled: use default Python eval frame.
    Disabled = 0,
    /// Enabled: use Sapling_PyEvalFrame (minimal overhead, no tracking).
    Enabled = 1,
    /// Probe: use Sapling_PyEvalFrameProbe (tracks last_frame for offset detection).
    Probe = 2,
}

/// Set the evalframe mode.
///
/// - `Disabled`: Use default Python eval frame.
/// - `Enabled`: Use `Sapling_PyEvalFrame` which keeps the frame state in its
///   stack frame for native debuggers, with minimal overhead.
/// - `Probe`: Use `Sapling_PyEvalFrameProbe` which also tracks the last frame
///   for offset detection at build time.
///
/// Note: calling this function when the Python interpreter is not initialized
/// is a no-op.
///
/// # Safety
/// This function is safe to call at any time, but should only be called
/// after Python initialization for the setting to take effect.
pub unsafe fn set_mode(mode: EvalFrameMode) {
    unsafe { sapling_cext_evalframe_set_mode(mode as libc::c_int) }
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
    pline_no: *mut isize,
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

/// Get the addresses of `Sapling_PyEvalFrame` and `Sapling_PyEvalFrameInner`.
///
/// This is used to identify Python frames in native stack traces by
/// comparing instruction pointers against known offsets from this address.
pub fn sapling_py_eval_frame_addr() -> usize {
    Sapling_PyEvalFrame as *const () as usize
}

/// Get the last (code, line_no) captured by `Sapling_PyEvalFrameProbe`.
///
/// This is useful for probing the PyFrame variable on the stack during
/// offset detection at build time.
pub fn get_last_code_line_no() -> (usize, isize) {
    unsafe {
        (
            sapling_cext_evalframe_get_last_code(),
            sapling_cext_evalframe_get_last_line_no(),
        )
    }
}

/// Get the last frame captured by `Sapling_PyEvalFrameProbe`.
/// This is only used for compatibility.
pub fn get_last_frame() -> usize {
    unsafe { sapling_cext_evalframe_get_last_frame() }
}

/// Read one machine word from a native stack address during offset probing.
///
/// # Safety
/// - `ptr` must be readable for `usize` bytes.
/// - `ptr` must be aligned to `align_of::<usize>()`: the C implementation
///   performs a typed `*(const volatile uintptr_t*)ptr` read, and a
///   readability check alone does not establish alignment.
/// - This is only intended for the bounded `Sapling_PyEvalFrame` stack scan
///   performed by the offset probe.
pub unsafe fn probe_read_stack_word(ptr: *const libc::c_void) -> usize {
    unsafe { sapling_cext_evalframe_probe_read_stack_word(ptr) }
}

// Raw FFI declarations for evalframe.c
unsafe extern "C" {
    fn sapling_cext_evalframe_set_mode(mode: libc::c_int);

    fn sapling_cext_evalframe_resolve_code_object(
        code: *mut libc::c_void,
        pfilename: *mut *const libc::c_char,
    ) -> *const libc::c_char;

    fn sapling_cext_evalframe_extract_code_lineno_from_frame(
        frame: *mut libc::c_void,
        pline_no: *mut isize,
    ) -> *mut libc::c_void;

    fn sapling_cext_evalframe_resolve_frame_is_supported() -> libc::c_int;

    fn sapling_cext_evalframe_resolve_frame(frame_ptr: usize) -> *const u8;

    fn sapling_cext_evalframe_get_last_frame() -> usize;
    fn sapling_cext_evalframe_get_last_code() -> usize;
    fn sapling_cext_evalframe_get_last_line_no() -> isize;

    fn sapling_cext_evalframe_probe_read_stack_word(ptr: *const libc::c_void) -> usize;

    // The pass-through eval frame function. We use its address and scans its stack.
    // It's not called directly.
    fn Sapling_PyEvalFrame(tstate: usize, f: usize, exc: libc::c_int);
}
