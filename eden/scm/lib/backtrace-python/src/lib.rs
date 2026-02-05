/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! `backtrace-ext` extension to support resolving Python frames.
//!
//! Call `init()` after Python initialization to attempt to enable Python frame
//! resolution. Not all platforms are supported. Check `SUPPORTED_INFO` for
//! whether it's supported or not.

use std::ffi::CStr;
use std::sync::LazyLock;

use backtrace_ext::FrameDecision;
use backtrace_ext::SupplementalFrameResolver;
use backtrace_ext::SupplementalInfo;

mod libpython_filter;
mod offsets;

/// Setup backtrace-ext to resolve Python frames on supported platforms.
/// This function is a no-op if the platform is not supported.
///
/// Calling this function when the Python interpreter is not initialized does
/// not complete the initialization. Call again after Python initialization.
pub fn init() {
    let is_supported = SUPPORTED_INFO.is_supported();
    if is_supported {
        static RESOLVER: PythonSupplementalFrameResolver = PythonSupplementalFrameResolver;
        static RESOLVER_FAT_REF: &dyn SupplementalFrameResolver = &RESOLVER;
        static RESOLVER_THIN_REF: &&dyn SupplementalFrameResolver = &RESOLVER_FAT_REF;
        libpython_filter::init();
        backtrace_ext::set_supplemental_frame_resolver(Some(RESOLVER_THIN_REF));
        unsafe {
            // This function is a no-op if called before Python initialization.
            evalframe_sys::set_mode(evalframe_sys::EvalFrameMode::Enabled);
            // keep the C function alive (for dbgutil.py lldb usage)
            evalframe_sys::resolve_frame(0);
        }
    }
}

/// Information about whether the Python frame resolution is supported or not.
/// All fields must be `true` to indicate support.
#[derive(Clone, Copy, Debug)]
pub struct SupportedInfo {
    /// Whether the (OS, architecture) combination is supported.
    /// Decided by whether the `offsets` can be detected at build time.
    pub os_arch: bool,
    /// Whether the C evalframe logic supports frame resolution.
    /// This is usually affected by the cpython version.
    pub c_evalframe: bool,
}

impl SupportedInfo {
    pub fn is_supported(&self) -> bool {
        self.os_arch && self.c_evalframe
    }

    fn new() -> Self {
        Self {
            os_arch: offsets::OFFSET_IP.is_some() && offsets::OFFSET_SP.is_some(),
            c_evalframe: evalframe_sys::resolve_frame_is_supported(),
        }
    }
}

pub static SUPPORTED_INFO: LazyLock<SupportedInfo> = LazyLock::new(SupportedInfo::new);

#[derive(Copy, Clone)]
struct PythonSupplementalFrameResolver;

impl SupplementalFrameResolver for PythonSupplementalFrameResolver {
    fn maybe_extract_supplemental_info(&self, ip: usize, sp: usize) -> FrameDecision {
        let Some(offset) = offsets::OFFSET_IP else {
            return FrameDecision::Keep;
        };
        if ip != (evalframe_sys::sapling_py_eval_frame_addr() + offset) {
            // Skip native python frames to reduce noise.
            return if libpython_filter::is_python_frame(ip) {
                FrameDecision::Skip
            } else {
                FrameDecision::Keep
            };
        }
        match extract_python_supplemental_info(sp) {
            Some(info) => FrameDecision::Replace(info),
            None => FrameDecision::Keep,
        }
    }

    fn resolve_supplemental_info(&self, info: &SupplementalInfo) -> Option<String> {
        let [code, line_no] = *info;
        unsafe {
            let mut filename_ptr: *const libc::c_char = std::ptr::null();
            let name_ptr =
                evalframe_sys::resolve_code_object(code as *mut libc::c_void, &mut filename_ptr);
            if !name_ptr.is_null() && !filename_ptr.is_null() {
                let name_cstr = CStr::from_ptr(name_ptr);
                let filename_cstr = CStr::from_ptr(filename_ptr);
                let desc = format!(
                    "{} at {}:{}",
                    name_cstr.to_string_lossy(),
                    filename_cstr.to_string_lossy(),
                    line_no
                );
                return Some(desc);
            }
        }
        None
    }
}

fn extract_python_supplemental_info(sp: usize) -> Option<SupplementalInfo> {
    if sp == 0 {
        return None;
    }
    // Read the `f` variable on stack. See sapling/dbgutil.py, D55728746
    let offset = offsets::OFFSET_SP?;
    let addr = sp.checked_add(offset)?;
    unsafe {
        let frame_ptr: *const *mut libc::c_void = addr as *const _;
        let frame: *mut libc::c_void = *frame_ptr;
        let mut line_no: libc::c_int = 0;
        let code = evalframe_sys::extract_code_lineno_from_frame(frame, &mut line_no);
        if !code.is_null() {
            return Some([code as usize, line_no as usize]);
        }
    }
    None
}
