// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This crate has utilities to deal with creation and operations on the bonsai data model.

//#![deny(warnings)]

use failure_ext as failure;

mod composite;
mod diff;

pub use crate::composite::{CompositeEntry, CompositeManifest};
pub use crate::diff::{bonsai_diff, BonsaiDiffResult};
