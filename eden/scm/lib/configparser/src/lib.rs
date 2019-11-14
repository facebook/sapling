/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # ConfigParser
//!
//! ConfigParser is a utility to parse hgrc-like config files.
//!
//! ## Features
//!
//! - Parse valid hgrc-like config files efficiently.
//! - Track source locations of config values. Keep multiple locations of
//!   a same config if it is overridden.
//!
//! ## Config Format
//!
//! hgrc files are similar to INI files:
//!
//! ```plain,ignore
//! [section1]
//! name1 = value1
//! name2 = value2
//!
//! [section2]
//! name3 = value3
//!
//! ; This is a comment.
//! # This is also a comment.
//! ```
//!
//! But with some additional features.
//!
//! ### Include other config files
//!
//! Use `%include` to include other config files:
//!
//! ```plain,ignore
//! %include path/to/another/hgrc
//! %include path/to/another/hgrc.d
//! ```
//!
//! The include path is relative to the directory of the current config
//! file being parsed. If it's a directory, files with names ending
//! with `.rc` in it will be read.
//!
//! ### Unset a config
//!
//! Use `%unset` to unset a config:
//!
//! ```plain,ignore
//! [section]
//! %unset name1
//! ```
//!
//! ### Multi-line values
//!
//! Indent non-first lines with a space:
//!
//! ```plain,ignore
//! [section]
//! name1 = value
//!  line2
//!  line3
//! ```

pub mod c_api;
pub mod config;
pub mod error;
pub mod hg;
pub mod parser;

pub use error::Error;
