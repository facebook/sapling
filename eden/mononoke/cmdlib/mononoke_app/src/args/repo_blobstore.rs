/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::ArgGroup;
use clap::Args;
use clap::ValueEnum;

/// Command line arguments for specifying a repo, the blobstore used will match the one in the repo config
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("repo_blobstore")
        .args(&["repo_id", "repo_name"]),
))]
pub struct RepoBlobstoreArgs {
    /// Numeric repository ID
    #[clap(long)]
    pub repo_id: Option<i32>,

    /// Repository name
    #[clap(short = 'R', long)]
    pub repo_name: Option<String>,

    /// If the blobstore is multiplexed, use this inner blobstore
    #[clap(long)]
    pub inner_blobstore_id: Option<u64>,

    /// Use memcache to cache access to the blobstore
    #[clap(long, value_enum)]
    pub use_memcache: Option<UseMemcache>,

    /// Bypass redaction when opening this blobstore
    #[clap(long)]
    pub bypass_redaction: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum, Eq, PartialEq)]
pub enum UseMemcache {
    CacheOnly,
    NoFill,
    FillMc,
}
