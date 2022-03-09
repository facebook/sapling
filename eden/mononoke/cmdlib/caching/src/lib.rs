/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod args;
#[cfg(fbcode_build)]
pub mod facebook;
mod settings;

use environment::Caching;
use fbinit::FacebookInit;

pub use crate::args::CachelibArgs;
pub use crate::settings::CachelibSettings;

pub fn init_cachelib(
    fb: FacebookInit,
    settings: &CachelibSettings,
    args: &CachelibArgs,
) -> Caching {
    if args.skip_caching {
        return Caching::Disabled;
    }

    let mut settings = settings.clone();
    settings.update_from_args(args);

    #[cfg(not(fbcode_build))]
    {
        let _ = fb;
        unimplemented!("Initialization of cachelib works only for fbcode builds")
    }
    #[cfg(fbcode_build)]
    {
        facebook::init_cachelib_from_settings(fb, settings)
            .expect("cachelib initialize should always succeed");

        if args.blobstore_cachelib_only {
            Caching::CachelibOnlyBlobstore(args.cachelib_shards)
        } else {
            Caching::Enabled(args.cachelib_shards)
        }
    }
}
