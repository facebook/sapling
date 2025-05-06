/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Track dependencies of values and invalidate/re-calculate accordingly.
//!
//! Inspired by [Jotai](https://jotai.org/). Note this library only implements
//! a small subset of Jotai features.

mod atom;

pub use anyhow::Result;
pub use atom::Atom;
pub use atom::GetAtomValue;
pub use atom::PrimitiveValue;
