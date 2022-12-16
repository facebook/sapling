/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
