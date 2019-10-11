/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Extensions for the bytes crate.

#![deny(warnings)]

mod bufext;
mod sized;

pub use crate::bufext::BufExt;
pub use crate::sized::SizeCounter;
