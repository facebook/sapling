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
mod export;
mod metalog;

pub use crate::metalog::resolver;
pub use crate::metalog::CommitOptions;
pub use crate::metalog::Id20;
pub use crate::metalog::MetaLog;
pub use errors::Error;
pub use errors::Result;
pub use indexedlog::Repair;
