/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides a non-lazy repo that serves as a "server" repo in tests.
//!
//! Main goals:
//! - Pure Rust. No Python dependencies.
//! - Serve as EdenApi without going through real networking stack.
//! - Replace SSH reps in tests, which is slow and unreliable on Windows.
//!
//! Although it's currently intended to be a test server repo. It is
//! in theory not too difficult to make it useful as a kind of small
//! client repo.
//!
//! [`EagerRepo`] is the main struct.
//!
//! The word "eager" comes from "eager evaluation", the opposite of
//! "lazy evaluation".

mod api;
mod eager_repo;
mod errors;
mod trait_impls;

pub use api::edenapi_from_config;
pub use eager_repo::EagerRepo;
pub use eager_repo::EagerRepoStore;
pub use errors::Error;
pub type Result<T> = std::result::Result<T, Error>;
