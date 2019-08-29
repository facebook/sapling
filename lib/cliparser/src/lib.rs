// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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
//!

pub mod alias;
pub mod errors;
pub mod macros;
pub mod parser;
pub mod utils;
