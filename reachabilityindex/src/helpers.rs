// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Error;
use futures::future::{err, join_all, ok, Future};
use futures_ext::{BoxFuture, FutureExt};

use blobrepo;
use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::Generation;

use errors::*;

/// Attempts to fetch the generation number of the hash. Succeeds with the Generation value
/// of the node if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn fetch_generation(repo: Arc<BlobRepo>, node: HgNodeHash) -> BoxFuture<Generation, Error> {
    repo.get_generation_number(&HgChangesetId::new(node.clone()))
        .map_err(|err| {
            ErrorKind::GenerationFetchFailed(BlobRepoErrorCause::new(
                err.downcast::<blobrepo::ErrorKind>().ok(),
            )).into()
        })
        .and_then(move |genopt| {
            genopt.ok_or_else(move || ErrorKind::NodeNotFound(format!("{}", node)).into())
        })
        .boxify()
}

/// Confirm whether or not a node with the given hash exists in the repo.
/// Succeeds with the void value () if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn check_if_node_exists(repo: Arc<BlobRepo>, node: HgNodeHash) -> BoxFuture<(), Error> {
    repo.changeset_exists(&HgChangesetId::new(node.clone()))
        .map_err(move |err| {
            ErrorKind::CheckExistenceFailed(
                format!("{}", node),
                BlobRepoErrorCause::new(err.downcast::<blobrepo::ErrorKind>().ok()),
            ).into()
        })
        .and_then(move |exists| {
            if exists {
                ok(())
            } else {
                err(ErrorKind::NodeNotFound(format!("{}", node.clone())).into())
            }
        })
        .boxify()
}

/// Convert a collection of HgChangesetId to a collection of (HgNodeHash, Generation)
pub fn changeset_to_nodehashes_with_generation_numbers(
    repo: Arc<BlobRepo>,
    nodes: Vec<HgChangesetId>,
) -> BoxFuture<Vec<(HgNodeHash, Generation)>, Error> {
    join_all(nodes.into_iter().map(move |node_cs| {
        repo.get_generation_number(&node_cs)
            .map_err(|err| {
                ErrorKind::GenerationFetchFailed(BlobRepoErrorCause::new(
                    err.downcast::<blobrepo::ErrorKind>().ok(),
                )).into()
            })
            .and_then(move |genopt| {
                genopt.ok_or_else(move || ErrorKind::NodeNotFound(format!("{}", node_cs)).into())
            })
            .map(move |gen_id| (*node_cs.as_nodehash(), gen_id))
    })).boxify()
}
