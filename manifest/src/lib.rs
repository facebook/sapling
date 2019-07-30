// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

pub use crate::derive::{derive_manifest, LeafInfo, TreeInfo};
pub use crate::ops::{Diff, ManifestOps};
pub use crate::types::{Entry, Manifest, PathTree};

mod derive;
mod ops;
mod types;

#[cfg(test)]
mod tests;
