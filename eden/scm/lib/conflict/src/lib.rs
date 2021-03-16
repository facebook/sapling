/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Check-in conflict support.
//!
//! Main idea comes from [Jujube](https://github.com/martinvonz/jj).

mod model;

pub use model::{CommitConflict, FileConflict, FileContext};
