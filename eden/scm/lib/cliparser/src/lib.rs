/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # CLI Parser
//!
//! CLI Parser is a utility to support the parsing of command-line arguments, commands,
//! subcommands, and flags.
//!
//! # About
//! CLI Parser is used both to declare flags and commands as well as parse command-line arguments
//! in a type-safe way.  Flags can have values associated with them, default values, or no value.
//!
//! # Goals
//! Having a simple, easy-to-use CLI Parser in native code allowing for fast execution, parsing,
//! and validation of command-line arguments.
//!
//! Having the flexibility of being able to dynamically load flags from an external source such
//! as other languages ( python ) or files ( configuration ).

pub mod alias;
// Re-export so define_flags! macro can reliably reference $crate::anyhow::Result instead
// of implicitly requiring the caller to have "anyhow" available.
pub use anyhow;
pub mod errors;
pub mod macros;
pub mod parser;
pub mod utils;
