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
        // This function is a no-op if called before Python initialization.
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
            os_arch: OFFSET.is_some(),
            c_evalframe: unsafe { sapling_cext_evalframe_resolve_frame_is_supported() } != 0,
        }
    }
}

pub static SUPPORTED_INFO: LazyLock<SupportedInfo> = LazyLock::new(SupportedInfo::new);

// for evalframe.c
unsafe extern "C" {
    fn sapling_cext_evalframe_set_pass_through(enabled: u8);

    unsafe fn sapling_cext_evalframe_resolve_code_object(
        code: *mut libc::c_void, /* PyCodeObject */
        pfilename: *mut *const libc::c_char,
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

/// Raw offsets.
/// When IP (program counter) is `OFFSET.0 + Sapling_PyEvalFrame`,
/// the `PyFrame` can be read at `OFFSET.1 + SP`.
const OFFSET: Option<(usize, usize)> = {
    if cfg!(all(
        any(target_os = "linux", target_os = "macos"),
        target_arch = "x86_64"
    )) {
        // Sapling_PyEvalFrame(PyThreadState* tstate, PyFrameObject* f, int exc)
        // (lldb) disassemble -n Sapling_PyEvalFrame
        // `Sapling_PyEvalFrame:
        //  <+0>:  pushq  %rbp
        //  <+1>:  movq   %rsp, %rbp        ; FP
        //  <+4>:  subq   $0x20, %rsp       ; SP = FP - 0x20
        //  <+8>:  movq   %rdi, -0x18(%rbp)
        //  <+12>: movq   %rsi, -0x10(%rbp) ; PyFrame f at FP - 0x10 or SP + 0x10
        //  <+16>: movl   %edx, -0x4(%rbp)
        //  <+19>: movq   -0x18(%rbp), %rdi
        //  <+23>: movq   -0x10(%rbp), %rsi
        //  <+27>: movl   -0x4(%rbp), %edx
        //  <+30>: callq  0x8d4eb0       ; symbol stub for: _PyEval_EvalFrameDefault
        //  <+35>: addq   $0x20, %rsp
        //  <+39>: popq   %rbp
        //  <+40>: retq
        Some((35, 0x10))
    } else if cfg!(all(
        any(target_os = "linux", target_os = "macos"),
        target_arch = "aarch64"
    )) {
        //  <+0>:  sub    sp, sp, #0x30
        //  <+4>:  stp    x29, x30, [sp, #0x20]
        //  <+8>:  add    x29, sp, #0x20      ; FP (x29) = SP + 0x20
        //  <+12>: stur   x0, [x29, #-0x8]    ; x0 is 1st arg (tstate)
        //  <+16>: str    x1, [sp, #0x10]     ; x1 is 2nd arg (f), at SP + 0x10
        //  <+20>: str    w2, [sp, #0xc]
        //  <+24>: ldur   x0, [x29, #-0x8]
        //  <+28>: ldr    x1, [sp, #0x10]
        //  <+32>: ldr    w2, [sp, #0xc]
        //  <+36>: bl     0x102c76340    ; symbol stub for: _PyEval_EvalFrameDefault
        //  <+40>: ldp    x29, x30, [sp, #0x20]
        //  <+44>: add    sp, sp, #0x30
        //  <+48>: ret
        Some((40, 0x10))
    } else if cfg!(all(
        target_os = "windows",
        target_env = "msvc",
        target_arch = "x86_64"
    )) {
        //  <+0>:   pushq  %rbp
        //  <+1>:   subq   $0x40, %rsp
        //  <+5>:   leaq   0x40(%rsp), %rbp  ; FP = SP + 0x40
        //  <+10>:  movl   %r8d, -0x4(%rbp)
        //  <+14>:  movq   %rdx, -0x18(%rbp) ; rdx is 2nd arg. FP - 0x18 = SP + 0x28
        //  <+18>:  movq   %rcx, -0x10(%rbp)
        //  <+22>:  movl   -0x4(%rbp), %r8d
        //  <+26>:  movq   -0x18(%rbp), %rdx
        //  <+30>:  movq   -0x10(%rbp), %rcx
        //  <+34>:  callq  *0x517e830(%rip)
        //  <+40>:  nop
        //  <+41>:  addq   $0x40, %rsp
        //  <+45>:  popq   %rbp
        //  <+46>:  retq
        Some((40, 0x28))
    } else {
        // Unsupported OS or arch.
        None
    }
};

impl SupplementalFrameResolver for PythonSupplementalFrameResolver {
    fn maybe_extract_supplemental_info(&self, ip: usize, sp: usize) -> FrameDecision {
        let offset: usize = match OFFSET {
            Some(o) => o.0,
            None => return FrameDecision::Keep,
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
            let mut filename_ptr: *const libc::c_char = std::ptr::null();
            let name_ptr = sapling_cext_evalframe_resolve_code_object(
                code as *mut libc::c_void,
                &mut filename_ptr,
            );
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
    let offset = OFFSET?.1;
    let addr = sp.checked_add(offset)?;
    unsafe {
        let frame_ptr: *const *mut libc::c_void = addr as *const _;
        let frame: *mut libc::c_void = *frame_ptr;
        let mut line_no: libc::c_int = 0;
        let code = sapling_cext_evalframe_extract_code_lineno_from_frame(frame, &mut line_no);
        if !code.is_null() {
            return Some([code as usize, line_no as usize]);
        }
    }
    None
}
