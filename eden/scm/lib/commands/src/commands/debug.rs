/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cmdutil::define_flags;

commands! {
    mod structuredprogress;
    mod scmstore;
    mod scmstorereplay;
    mod segmentclone;
    mod segmentgraph;
    mod segmentpull;
    mod store;
    mod top;
    mod wait;
}

define_flags! {
    pub struct DebugArgsOpts {
        #[args]
        args: Vec<String>,
    }
}
