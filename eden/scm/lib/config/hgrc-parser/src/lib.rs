/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # Parsing hgrc content using pest.
//!
//! Parse hgrc content (`str`) into a list of instructions:
//! - SetConfig(section, name, value)
//! - UnsetConfig(section, name)
//! - Include(path)
//!
//! Pure. Do not depend on a filesystem.

pub(crate) mod config;
#[cfg(test)]
mod tests;

pub use config::parse;
pub use config::Instruction;
