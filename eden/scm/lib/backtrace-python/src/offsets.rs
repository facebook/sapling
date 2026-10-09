/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Offsets for extracting Python frames from native stack traces.
//!
//! This file exists to make static analysis work.
//! Both cargo and buck build will re-generate this file at build time.

/// IP (PC) Offset in `Sapling_PyEvalFrame` after
/// `call _PyEval_EvalFrameDefault`.
pub const OFFSET_IP: Option<usize> = None;

/// SP Offset to get the interpreter frame.
/// Note: it might be de-allocated during Py_EvalFrame!
pub const OFFSET_SP_FRAME: Option<usize> = None;

/// SP Offset to get the PyCodeObject.
pub const OFFSET_SP_CODE: Option<usize> = None;

/// SP Offset to get the isize line_no.
pub const OFFSET_SP_LINE_NO: Option<usize> = None;

/// SP Offset to get the cookie bound to its address, code and line number.
pub const OFFSET_SP_COOKIE: Option<usize> = None;
