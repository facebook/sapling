/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # ConfigSet
//!
//! `ConfigSet` provides a mutable Config abstraction that has the ability to
//! load files in hgrc-format.
//!
//! If you only need to take a config object and read values from it, use
//! `configmodel::Config` and `configmodel::ConfigExt` instead.
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

mod builtin;
pub mod config;

pub use configmodel;
pub use configmodel::convert;
pub use configmodel::error;
pub use configmodel::Config;
pub use configmodel::Error;
pub use configmodel::Result;
pub use error::Errors;
// Re-export
pub use minibytes::Text;
