/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Data model related to progress reporting.
//!
//! - Pure data. Minimal state just enough for rendering.
//! - Separate from rendering.
//! - Lock-free (nice to have).

mod io_sample;

pub use io_sample::IoSample;
