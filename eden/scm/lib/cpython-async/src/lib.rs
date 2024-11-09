/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(non_camel_case_types)]

//! Utilities to make async <-> Python integration easier.
//!
//! The `TStream` type provides easy conversion between Rust `Stream` and Python
//! objects.
//!
//! The `PyFuture` type provides a way to export Rust `Future` to
//! Python.

mod future;
mod stream;

// Re-export.
pub use anyhow;
pub use async_runtime;
pub use cpython;
pub use cpython_ext;
pub use future::future as PyFuture;
pub use futures;
pub use stream::TStream;
