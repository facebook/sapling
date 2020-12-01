/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use super::define_flags;
pub use super::ConfigSet;
pub use super::NoOpts;
pub use super::Repo;
pub use super::Result;
pub use super::IO;

commands! {
    mod args;
    mod causerusterror;
    mod dumpindexedlog;
    mod dumptrace;
    mod dynamicconfig;
    mod fsync;
    mod http;
    mod python;
    mod segmentclone;
    mod store;
}

define_flags! {
    pub struct DebugArgsOpts {
        #[args]
        args: Vec<String>,
    }
}
