/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! A center place to call `init` from various crates.

use once_cell::sync::Lazy;

/// Register constructors.
pub fn init() {
    static REGISTERED: Lazy<()> = Lazy::new(|| {
        // File, tree stores.
        #[cfg(feature = "git")]
        gitstore::init();
        eagerepo::init();

        // Commit stores.
        commits::init();
        #[cfg(feature = "git")]
        commits_git::init();

        // Remote peers.
        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config);

        // Basic tree parser.
        manifest_tree::init();
    });

    *REGISTERED
}
