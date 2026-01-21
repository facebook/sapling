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

/// Setup backtrace-ext to resolve Python frames on supported platforms.
/// Python interpreter must be initialized at this time.
/// This function is a no-op if the platform is not supported.
pub fn init() {
    let is_supported = SUPPORTED_INFO.is_supported();
    if is_supported {
        static RESOLVER: PythonSupplementalFrameResolver = PythonSupplementalFrameResolver;
        static RESOLVER_FAT_REF: &dyn SupplementalFrameResolver = &RESOLVER;
        static RESOLVER_THIN_REF: &&dyn SupplementalFrameResolver = &RESOLVER_FAT_REF;
        libpython_filter::init();
        backtrace_ext::set_supplemental_frame_resolver(RESOLVER_THIN_REF);
        unsafe { sapling_cext_evalframe_set_pass_through(1) }
    }
}

/// Information about whether the Python frame resolution is supported or not.
/// All fields must be `true` to indicate support.
#[derive(Clone, Copy, Debug)]
pub struct SupportedInfo {
    /// Whether the (OS, architecture) combination is supported.
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
            os_arch: cfg!(all(target_os = "linux", target_arch = "x86_64")),
            c_evalframe: unsafe { sapling_cext_evalframe_resolve_frame_is_supported() } != 0,
        }
    }
}

pub static SUPPORTED_INFO: LazyLock<SupportedInfo> = LazyLock::new(SupportedInfo::new);

// for evalframe.c
unsafe extern "C" {
    fn sapling_cext_evalframe_set_pass_through(enabled: u8);
    fn sapling_cext_evalframe_stringify_code_lineno(
        code: *mut libc::c_void, /* PyCodeObject */
        line_no: libc::c_int,
    ) -> *const libc::c_char;
    fn sapling_cext_evalframe_extract_code_lineno_from_frame(
        frame: *mut libc::c_void, /* PyFrame */
        pline_no: *mut libc::c_int,
    ) -> *mut libc::c_void /* PyCodeObject */;
    fn sapling_cext_evalframe_resolve_frame_is_supported() -> libc::c_int;

    // only need the function address, no need to call this function
    fn Sapling_PyEvalFrame(tstate: usize, f: usize, exc: libc::c_int);
}

#[derive(Copy, Clone)]
struct PythonSupplementalFrameResolver;

impl SupplementalFrameResolver for PythonSupplementalFrameResolver {
    fn maybe_extract_supplemental_info(&self, ip: usize, sp: usize) -> FrameDecision {
        let offset: usize = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            // (lldb) disassemble -n Sapling_PyEvalFrame
            // example`Sapling_PyEvalFrame:
            // example[0x4be510] <+0>:  pushq  %rbp
            // example[0x4be511] <+1>:  movq   %rsp, %rbp
            // example[0x4be514] <+4>:  subq   $0x20, %rsp
            // example[0x4be518] <+8>:  movq   %rdi, -0x18(%rbp)
            // example[0x4be51c] <+12>: movq   %rsi, -0x10(%rbp)
            // example[0x4be520] <+16>: movl   %edx, -0x4(%rbp)
            // example[0x4be523] <+19>: movq   -0x18(%rbp), %rdi
            // example[0x4be527] <+23>: movq   -0x10(%rbp), %rsi
            // example[0x4be52b] <+27>: movl   -0x4(%rbp), %edx
            // example[0x4be52e] <+30>: callq  0x8d4eb0       ; symbol stub for: _PyEval_EvalFrameDefault
            // example[0x4be533] <+35>: addq   $0x20, %rsp
            // example[0x4be537] <+39>: popq   %rbp
            // example[0x4be538] <+40>: retq
            35
        } else {
            // Unsupported OS or arch.
            return FrameDecision::Keep;
        };
        if ip != (Sapling_PyEvalFrame as usize + offset) {
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
            let desc = sapling_cext_evalframe_stringify_code_lineno(
                code as *mut libc::c_void,
                line_no as libc::c_int,
            );
            if !desc.is_null() {
                let c_str = CStr::from_ptr(desc);
                return Some(c_str.to_string_lossy().into_owned());
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
    // Sapling_PyEvalFrame(PyThreadState* tstate, PyFrameObject* f, int exc)
    //
    // x64:
    //   pushq  %rbp
    //   movq   %rsp, %rbp        ; FP
    //   subq   $0x20, %rsp       ; SP = FP - 0x20
    //   movq   %rdi, -0x8(%rbp)
    //   movq   %rsi, -0x10(%rbp) ; PyFrame f at FP - 0x10, or SP + 0x10
    //   movl   %edx, -0x14(%rbp)
    //   movq   -0x8(%rbp), %rdi
    //   movq   -0x10(%rbp), %rsi
    //   movl   -0x14(%rbp), %edx
    //   callq  0x1034bddee       ; symbol stub for: _PyEval_EvalFrameDefault
    //   addq   $0x20, %rsp
    //   popq   %rbp
    //   retq
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        let addr = sp.checked_add(0x10)?;
        unsafe {
            let frame_ptr: *const *mut libc::c_void = addr as *const _;
            let frame: *mut libc::c_void = *frame_ptr;
            let mut line_no: libc::c_int = 0;
            let code = sapling_cext_evalframe_extract_code_lineno_from_frame(frame, &mut line_no);
            if !code.is_null() {
                return Some([code as usize, line_no as usize]);
            }
        }
    }
    None
}
