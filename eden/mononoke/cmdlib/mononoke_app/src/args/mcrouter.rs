/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use environment::MononokeEnvironment;

use crate::AppExtension;

/// Command line arguments that affect mcrouter usage
#[derive(Args, Debug)]
pub struct McrouterArgs {
    /// Use local McRouter for rate limits
    #[clap(long)]
    pub enable_mcrouter: bool,

    /// Override the number of threads used for McRouter
    #[clap(long)]
    pub num_mcrouter_proxy_threads: Option<usize>,

    /// Override the maximum number of outstanding Memcache requests
    #[clap(long)]
    pub memcache_max_outstanding_requests: Option<usize>,
}

pub struct McrouterAppExtension;

impl AppExtension for McrouterAppExtension {
    type Args = McrouterArgs;

    fn environment_hook(&self, args: &McrouterArgs, env: &mut MononokeEnvironment) -> Result<()> {
        if let Some(count) = args.num_mcrouter_proxy_threads {
            memcache::set_proxy_threads_count(count)?;
        }

        if let Some(count) = args.memcache_max_outstanding_requests {
            memcache::set_max_outstanding_requests(count)?;
        }

        if args.enable_mcrouter {
            #[cfg(fbcode_build)]
            {
                ::ratelim::use_proxy_if_available(env.fb);
            }

            #[cfg(not(fbcode_build))]
            {
                let _ = env;
                unimplemented!(
                    "Passed --enable-mcrouter but it is supported only for fbcode builds",
                );
            }
        }

        Ok(())
    }
}
