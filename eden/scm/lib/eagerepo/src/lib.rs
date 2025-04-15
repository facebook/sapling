/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Provides a non-lazy repo that serves as a "server" repo in tests.
//!
//! Main goals:
//! - Pure Rust. No Python dependencies.
//! - Serve as SaplingRemoteApi without going through real networking stack.
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
mod cas;
mod eager_repo;
mod errors;
mod factory_impls;
mod trait_impls;

pub use api::edenapi_from_config;
pub use eager_repo::EagerRepo;
pub use eager_repo::EagerRepoStore;
pub use eager_repo::is_eager_repo;
pub use errors::Error;
pub type Result<T> = std::result::Result<T, Error>;

/// Initialization. Register abstraction implementations.
pub fn init() {
    crate::factory_impls::init();
}
