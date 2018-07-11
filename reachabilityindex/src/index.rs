// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Error;
use futures::future::Future;

use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;

/// Trait for any method of supporting reachability queries
pub trait ReachabilityIndex {
    /// Return a Future for whether the src node can reach the dst node
    fn query_reachability(
        &mut self,
        repo: Arc<BlobRepo>,
        src: HgNodeHash,
        dst: HgNodeHash,
    ) -> Box<Future<Item = bool, Error = Error>>;
}
