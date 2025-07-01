/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

#![deny(warnings, missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::needless_doctest_main,
    clippy::new_without_default,
    elided_lifetimes_in_paths
)]
//! Provides [FacebookInit] structure that must be used in Facebook code that
//! requires pre-initialization, e.g. like C++'s logging.

#[cfg(not(fbcode_build))]
mod oss;

pub use fbinit_macros::main;
pub use fbinit_macros::nested_test;
pub use fbinit_macros::test;
#[cfg(not(fbcode_build))]
pub use oss::*;
#[cfg(fbcode_build)]
pub use real_fbinit::*;
