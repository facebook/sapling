/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
