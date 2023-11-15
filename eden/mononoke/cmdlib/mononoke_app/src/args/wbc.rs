/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use environment::BookmarkCacheKind;
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
        if let Some(derived_data) = args.enable_wbc_with {
            // Enable_wbc_with enables local cache by default
            if env.bookmark_cache_options.cache_kind == BookmarkCacheKind::Disabled {
                env.bookmark_cache_options.cache_kind = BookmarkCacheKind::Local;
            }
            // Derived data type is set regardless of the cache kind
            env.bookmark_cache_options.derived_data = derived_data;
        }
        Ok(())
    }
}
