/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Diff feature crate for isolating xdiff functionality
//!
//! This crate provides unified diff and related functionality, abstracting
//! the underlying xdiff library to provide compatibility with the diff service
//! and future migration paths.

pub mod operations;
pub mod types;
pub mod utils;
