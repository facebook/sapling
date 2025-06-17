/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! Provided SharedError wrapper for cloneable Error.
#![deny(warnings, missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]

/// Module containing SharedError that works well with anyhow::Error.
/// Similarly to anyhow, it hiddes the underlyin Error type.
pub mod anyhow;
/// Module containing SharedError generic type that doesn't work well with
/// anyhow, but doesn't hide the underlying error type.
pub mod std;
