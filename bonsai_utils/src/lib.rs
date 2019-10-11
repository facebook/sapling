/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! This crate has utilities to deal with creation and operations on the bonsai data model.

//#![deny(warnings)]

use failure_ext as failure;

mod composite;
mod diff;

pub use crate::composite::{CompositeEntry, CompositeManifest};
pub use crate::diff::{bonsai_diff, BonsaiDiffResult};
