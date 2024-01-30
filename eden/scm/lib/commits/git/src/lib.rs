/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;
mod factory_impls;
mod git;
mod utils;

/// Initialization. Register abstraction implementations.
pub fn init() {
    factory_impls::setup_commits_constructor();
}
