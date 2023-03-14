/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities interacting with hg store.

mod filestore_util;

pub use filestore_util::separate_metadata;
pub use filestore_util::strip_metadata;
