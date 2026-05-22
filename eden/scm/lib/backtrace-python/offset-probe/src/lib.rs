/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use core::iter::Iterator;
use std::sync::LazyLock;
use std::sync::OnceLock;

use cpython::*;
use evalframe_sys::EvalFrameMode;

pub fn get_offsets_code() -> String {
    let offsets = get_offsets();

    format!(
        r#"/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// @{}enerated by offset-codegen. Do not edit.

/// IP (PC) Offset in `Sapling_PyEvalFrame` after
/// `call _PyEval_EvalFrameDefault`.
pub const OFFSET_IP: Option<usize> = {:?};

/// SP Offset to get the interpreter frame.
/// Note: it might be de-allocated during Py_EvalFrame!
pub const OFFSET_SP_FRAME: Option<usize> = {:?};

/// SP Offset to get the PyCodeObject.
pub const OFFSET_SP_CODE: Option<usize> = {:?};

/// SP Offset to get the isize line_no.
pub const OFFSET_SP_LINE_NO: Option<usize> = {:?};
"#,
        "g",
        offsets.and_then(|o| o.ip_offset.get()),
        offsets.and_then(|o| o.sp_frame.get()),
        offsets.and_then(|o| o.sp_code.get()),
        offsets.and_then(|o| o.sp_line_no.get()),
    )
}

// See get_offsets_code for meanings of each field.
#[derive(Debug, Default)]
struct Offsets {
    ip_offset: OnceLock<usize>,
    sp_frame: OnceLock<usize>,
    sp_code: OnceLock<usize>,
    sp_line_no: OnceLock<usize>,
}

impl Offsets {
    fn is_fully_set(&self) -> bool {
        self.ip_offset.get().is_some()
            && self.sp_code.get().is_some()
            && self.sp_line_no.get().is_some()
    }
}

static OFFSETS: LazyLock<Offsets> = LazyLock::new(Offsets::default);

/// Attempt to get the variable names, and IP and SP offsets.
fn get_offsets() -> Option<&'static Offsets> {
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

    // Use padding to make `line_no` more "unique".
    let padding = "\n".repeat(7916);
    let code = format!(
        "{}{}",
        padding,
        r#"
def call_native_examine_backtrace():
    examine_backtrace()
call_native_examine_backtrace()
"#
    );
    py.run(&code, Some(&m.dict(py)), None).ok()?;

    if OFFSETS.is_fully_set() {
        Some(&*OFFSETS)
    } else {
        None
    }
}

/// Attempt to set OFFSET_IP, OFFSET_SP_CODE and OFFSET_SP_LINE_NO.
/// Intended to be called from a pure Python function.
fn examine_backtrace(_py: Python) -> PyResult<Option<bool>> {
    backtrace::trace(|frame| {
        let ip = frame.ip() as usize;
        let mut func_name = String::new();
        backtrace::resolve_frame(frame, |sym| {
            if let Some(name) = sym.name().and_then(|n| n.as_str()) {
                func_name = name.to_string();
            }
        });
        // How many bytes the `Sapling_PyEvalFrameInner` stack might be at most?
        const STACK_SIZE_THRESHOLD: usize = 48;

        // On macOS, backtrace may resolve names either from DWARF frames (often
        // "Sapling_PyEvalFrame") or from Mach-O symtab fallback (raw nlist name
        // "_Sapling_PyEvalFrame"), so we match both forms.
        if func_name == "Sapling_PyEvalFrame" || func_name == "_Sapling_PyEvalFrame" {
            let sp = frame.sp() as usize;
            let (last_code, last_line_no) = evalframe_sys::get_last_code_line_no();
            let last_frame = evalframe_sys::get_last_frame();

            for sp_offset in (0..=STACK_SIZE_THRESHOLD).step_by(std::mem::size_of::<usize>()) {
                let stack_ptr: *const *mut libc::c_void = (sp + sp_offset) as *const _;
                let value: *mut libc::c_void = unsafe { *stack_ptr };

                let mut has_code = OFFSETS.sp_code.get().is_some();
                let mut has_line_no = OFFSETS.sp_line_no.get().is_some();
                let mut has_frame = OFFSETS.sp_frame.get().is_some();

                if !has_code && value as usize == last_code {
                    OFFSETS.sp_code.get_or_init(|| sp_offset);
                    has_code = true;
                }
                if !has_line_no && value as isize == last_line_no {
                    OFFSETS.sp_line_no.get_or_init(|| sp_offset);
                    has_line_no = true;
                }
                if !has_frame && value as usize == last_frame {
                    OFFSETS.sp_frame.get_or_init(|| sp_offset);
                    has_frame = true;
                }
                if has_code && has_line_no {
                    // Got both SP offsets.
                    let start = evalframe_sys::sapling_py_eval_frame_addr();
                    OFFSETS.ip_offset.get_or_init(|| ip - start);
                }
            }
        }

        // Check the next frame if we haven't got all offsets.
        !OFFSETS.is_fully_set()
    });
    Ok(None)
}
