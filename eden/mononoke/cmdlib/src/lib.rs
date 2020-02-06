/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![deny(warnings)]
#![feature(never_type)]

pub mod args;
pub mod helpers;
mod log;
pub mod monitoring;
