/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # pathhistory
//!
//! This crate provides file or directory history algorithms that does not
//! depend on per-path indexes, is better than scanning commits one by one,
//! and is friendly for lazy stores.
//!
//! The basic idea is to use the segment struct from the `dag` crate and
//! (aggressively) skip large chunks of commits without visiting all commits
//! one by one.
//!
//! This might miss some "change then revert" commits but practically it
//! might be good enough.
//!
//! See `PathHistory` for the main structure.

#[allow(unused)]
mod pathops;

#[cfg(test)]
dev_logger::init!();
