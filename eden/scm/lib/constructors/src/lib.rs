/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
        hgcommits::init();
        #[cfg(feature = "git")]
        commits_git::init();

        // Remote peers.
        edenapi::Builder::register_customize_build_func(eagerepo::edenapi_from_config);

        // Basic tree parser.
        manifest_tree::init();
    });

    *REGISTERED
}
