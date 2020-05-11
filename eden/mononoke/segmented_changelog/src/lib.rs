/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![deny(warnings)]

///! segmented_changelog
///!
///! Data structures and algorithms for a commit graph used by source control.
pub mod dag;
mod idmap;
