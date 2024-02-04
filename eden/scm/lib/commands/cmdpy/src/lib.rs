/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
//! Initialize the Python interpreter with connections to the Rust commands and bindings.

mod hgpython;
mod python;
pub use hgpython::prepare_builtin_modules;
pub use hgpython::HgPython;
pub use hgpython::RustCommandConfig;
