/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use environment::MononokeEnvironment;
use environment::WarmBookmarksCacheDerivedData;

use crate::AppExtension;

/// Command line argument for Warm Bookmarks Cache
#[derive(Parser, Debug)]
pub struct WarmBookmarksCacheArgs {
    /// What needs to be warmed
    #[clap(long, value_enum, value_name = "warm derived data")]
    pub enable_wbc_with: Option<WarmBookmarksCacheDerivedData>,
}

pub struct WarmBookmarksCacheExtension;

impl AppExtension for WarmBookmarksCacheExtension {
    type Args = WarmBookmarksCacheArgs;

    fn environment_hook(&self, args: &Self::Args, env: &mut MononokeEnvironment) -> Result<()> {
        if let Some(wbc_config) = args.enable_wbc_with {
            env.warm_bookmarks_cache_derived_data = Some(wbc_config)
        }
        Ok(())
    }
}
