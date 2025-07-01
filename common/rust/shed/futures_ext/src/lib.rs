/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

#![deny(missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]

//! Crate extending functionality of [`futures`] crate

pub mod future;
pub mod stream;

pub use crate::future::FbFutureExt;
pub use crate::future::FbTryFutureExt;
pub use crate::stream::BufferedParams;
pub use crate::stream::FbStreamExt;
pub use crate::stream::FbTryStreamExt;
