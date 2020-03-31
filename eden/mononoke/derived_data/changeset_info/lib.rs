/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod changeset_info;
mod derive;

pub use crate::changeset_info::{ChangesetInfo, ChangesetMessage};
pub use crate::derive::ChangesetInfoMapping;
