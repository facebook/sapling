/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use core::iter::Iterator;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use cpython::*;
use evalframe_sys::EvalFrameMode;

/// IP (PC) Offset in Sapling_PyEvalFrame after the `call ...`.
static OFFSET_IP: AtomicUsize = AtomicUsize::new(0);

/// SP Offset to get the Python frame object.
static OFFSET_SP: AtomicUsize = AtomicUsize::new(0);

/// Attempt to get the IP and SP offsets.
pub fn get_offsets() -> Option<(usize, usize)> {
    // Unsupported Python version (Python C API)?
    if !evalframe_sys::resolve_frame_is_supported() {
        return None;
    }

    let gil = Python::acquire_gil();
    let py = gil.python();

    // Use probe mode to track last_frame for offset detection.
    unsafe { evalframe_sys::set_mode(EvalFrameMode::Probe) };

    let m = PyModule::new(py, "probe").ok()?;
    m.add(py, "examine_backtrace", py_fn!(py, examine_backtrace()))
        .ok()?;

    let code = r#"
def call_native_examine_backtrace():
    examine_backtrace()
call_native_examine_backtrace()
"#;
    py.run(code, Some(&m.dict(py)), None).ok()?;

    let offset_ip = OFFSET_IP.load(Ordering::Acquire);
    let offset_sp = OFFSET_SP.load(Ordering::Acquire);
    if offset_ip > 0 {
        Some((offset_ip, offset_sp))
    } else {
        None
    }
}

/// Attempt to set OFFSET_IP and OFFSET_SP.
/// Intended to be called from a pure Python function.
fn examine_backtrace(_py: Python) -> PyResult<Option<bool>> {
    backtrace::trace(|frame| {
        let ip = frame.ip() as usize;
        let start = evalframe_sys::sapling_py_eval_frame_addr();
        // How many bytes the `Sapling_PyEvalFrame` function has at most?
        const CODE_SIZE_THRESHOLD: usize = 64;
        // How many bytes the `Sapling_PyEvalFrame` stack might be at most?
        const STACK_SIZE_THRESHOLD: usize = 48;
        if ip >= start && ip <= start + CODE_SIZE_THRESHOLD {
            let sp = frame.sp() as usize;
            let last_frame = evalframe_sys::get_last_frame();

            // Try "known" SP offsets first. This reduces issues reading "bad" offsets.
            let sp_offset_hints: &[usize] = if cfg!(all(
                any(target_os = "linux", target_os = "macos"),
                any(target_arch = "x86_64", target_arch = "aarch64"),
            )) {
                // x86_64
                // Sapling_PyEvalFrame(PyThreadState* tstate, PyFrameObject* f, int exc)
                // (lldb) disassemble -n Sapling_PyEvalFrame
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
                //
                // x86_64 with FCF protection:
                //  <+0>:  endbr64
                //  <+4>:  pushq  %rbp
                //  <+5>:  movq   %rsp, %rbp
                //  <+8>:  subq   $0x20, %rsp
                //  <+12>: movq   %rdi, -0x8(%rbp)
                //  <+16>: movq   %rsi, -0x10(%rbp) ; PyFrame f at SP + 0x10
                //  <+20>: movl   %edx, -0x14(%rbp)
                //  <+23>: movl   -0x14(%rbp), %edx
                //  <+26>: movq   -0x10(%rbp), %rcx
                //  <+30>: movq   -0x8(%rbp), %rax
                //  <+34>: movq   %rcx, %rsi
                //  <+37>: movq   %rax, %rdi
                //  <+40>: callq  0x4e5e0b0      ; symbol stub for: _PyEval_EvalFrameDefault
                //  <+45>: leave
                //  <+46>: retq
                //
                // aarch64
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
                &[0x10]
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
                &[0x28]
            } else {
                // Unsupported OS or arch.
                &[]
            };

            for sp_offset in sp_offset_hints
                .iter()
                .copied()
                .chain((0..=STACK_SIZE_THRESHOLD).step_by(std::mem::size_of::<usize>()))
            {
                let frame_ptr: *const *mut libc::c_void = (sp + sp_offset) as *const _;
                let frame: *mut libc::c_void = unsafe { *frame_ptr };
                if frame as usize == last_frame {
                    // Got the SP offset and IP offset.
                    OFFSET_IP.store(ip - start, Ordering::Release);
                    OFFSET_SP.store(sp_offset, Ordering::Release);
                    return false;
                }
            }
        }
        // Not Sapling_PyEvalFrame. Check the next frame.
        true
    });
    Ok(None)
}
