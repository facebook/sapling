/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

pub mod errors;
pub mod nodemap;
mod revlogindex;

pub use crate::errors::RevlogIndexError as Error;
pub use crate::nodemap::NodeRevMap;
pub use crate::revlogindex::RevlogEntry;
pub use crate::revlogindex::RevlogIndex;
pub type Result<T> = std::result::Result<T, Error>;
