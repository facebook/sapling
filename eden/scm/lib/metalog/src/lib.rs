/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # metalog
//!
//! See [`MetaLog`] for the main structure.

mod errors;
mod metalog;

pub use errors::{Error, Result};
pub use metalog::{CommitOptions, Id20, MetaLog};
