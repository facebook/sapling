/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use environment::BookmarkCacheAddress;
use environment::BookmarkCacheDerivedData;
use environment::BookmarkCacheKind;
use environment::MononokeEnvironment;

use crate::AppExtension;

/// Command line argument for Warm Bookmarks Cache
#[derive(Parser, Debug)]
pub struct WarmBookmarksCacheArgs {
    /// What needs to be warmed
    #[clap(long, value_enum, value_name = "WARMING_MODE")]
    pub enable_wbc_with: Option<BookmarkCacheDerivedData>,

    #[clap(long, value_name = "BOOL")]
    pub use_remote_bookmark_cache: Option<bool>,

    /// Specify SMC tier for the derived data service
    #[clap(long, value_name = "SMC", group = "Bookmark Cache Address")]
    pub remote_bookmark_cache_tier: Option<String>,

    /// Specify Host:Port pair to connect to derived data service
    #[clap(long, value_name = "HOST:PORT", group = "Bookmark Cache Address")]
    pub remote_bookmark_cache_host_port: Option<String>,
}

pub struct WarmBookmarksCacheExtension;

impl AppExtension for WarmBookmarksCacheExtension {
    type Args = WarmBookmarksCacheArgs;

    /// This functions overrides the warm bookmark cache options in the environment from
    /// the CLI arguments. It allows for easy experimentation but for production enabling
    /// should be done via with_warm_bookmarks_cache on Mononoke App Builder.
    fn environment_hook(&self, args: &Self::Args, env: &mut MononokeEnvironment) -> Result<()> {
        // Parse the address from CLI arguments
        let address = if let Some(host_port) = args.remote_bookmark_cache_host_port.as_ref() {
            Some(BookmarkCacheAddress::HostPort(host_port.to_string()))
        } else {
            args.remote_bookmark_cache_tier
                .as_ref()
                .map(|smc_tier| BookmarkCacheAddress::SmcTier(smc_tier.to_string()))
        };
        if let Some(derived_data) = args.enable_wbc_with {
            // Enable_wbc_with enables local cache by default
            if env.bookmark_cache_options.cache_kind == BookmarkCacheKind::Disabled {
                env.bookmark_cache_options.cache_kind = BookmarkCacheKind::Local;
            }
            // Derived data type is set regardless of the cache kind
            env.bookmark_cache_options.derived_data = derived_data;
        }
        // Remote bookmark cache is enabled as an opt-in
        if args.use_remote_bookmark_cache == Some(true) {
            env.bookmark_cache_options.cache_kind =
                BookmarkCacheKind::Remote(address.clone().unwrap_or_default())
        }
        // User can also opt out of remote bookmark cache
        if args.use_remote_bookmark_cache == Some(false) {
            env.bookmark_cache_options.cache_kind = BookmarkCacheKind::Local;
        }

        // If remote bookmark cache was already enabled via other means override the address if
        // present.
        if let Some(new_address) = address {
            if let BookmarkCacheKind::Remote(address) = &mut env.bookmark_cache_options.cache_kind {
                *address = new_address;
            }
        }
        Ok(())
    }
}
