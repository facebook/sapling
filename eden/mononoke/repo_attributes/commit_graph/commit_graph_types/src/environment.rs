/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;

#[derive(Clone, Debug)]
pub struct CommitGraphOptions {
    pub skip_preloading_commit_graph: bool,
}

/// Command line arguments for configuring the commit graph.
#[derive(Args, Clone, Debug)]
pub struct CommitGraphArgs {
    /// Skip preloading the commit graph.
    #[clap(long, default_value_t = false)]
    pub skip_preloading_commit_graph: bool,
}

impl From<CommitGraphArgs> for CommitGraphOptions {
    fn from(args: CommitGraphArgs) -> Self {
        CommitGraphOptions {
            skip_preloading_commit_graph: args.skip_preloading_commit_graph,
        }
    }
}
