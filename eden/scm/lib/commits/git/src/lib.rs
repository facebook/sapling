/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod errors;
mod factory_impls;
mod git;
pub(crate) mod ref_filter;
pub(crate) mod ref_matcher;
mod utils;

/// Initialization. Register abstraction implementations.
pub fn init() {
    factory_impls::setup_commits_constructor();
}
