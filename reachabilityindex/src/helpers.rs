// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Error;
use futures::future::{err, join_all, ok, Future};

use blobrepo::{self, BlobRepo};
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::Generation;

use errors::*;

/// Attempts to fetch the generation number of the hash. Succeeds with the Generation value
/// of the node if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn fetch_generation(
    repo: Arc<BlobRepo>,
    node: HgNodeHash,
) -> impl Future<Item = Generation, Error = Error> {
    repo.get_generation_number(&HgChangesetId::new(node.clone()))
        .map_err(|err| {
            ErrorKind::GenerationFetchFailed(BlobRepoErrorCause::new(
                err.downcast::<blobrepo::ErrorKind>().ok(),
            )).into()
        })
        .and_then(move |genopt| {
            genopt.ok_or_else(move || ErrorKind::NodeNotFound(format!("{}", node)).into())
        })
}

pub fn fetch_generation_and_join(
    repo: Arc<BlobRepo>,
    node: HgNodeHash,
) -> impl Future<Item = (HgNodeHash, Generation), Error = Error> {
    fetch_generation(repo, node).map(move |gen| (node, gen))
}
/// Confirm whether or not a node with the given hash exists in the repo.
/// Succeeds with the void value () if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn check_if_node_exists(
    repo: Arc<BlobRepo>,
    node: HgNodeHash,
) -> impl Future<Item = (), Error = Error> {
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
}

/// Convert a collection of HgChangesetId to a collection of (HgNodeHash, Generation)
pub fn changeset_to_nodehashes_with_generation_numbers(
    repo: Arc<BlobRepo>,
    nodes: Vec<HgChangesetId>,
) -> impl Future<Item = Vec<(HgNodeHash, Generation)>, Error = Error> {
    join_all(nodes.into_iter().map({
        cloned!(repo);
        move |node_cs| fetch_generation_and_join(repo.clone(), *node_cs.as_nodehash())
    }))
}

/// Attempt to get the changeset parents of a hash node,
/// and cast into the appropriate ErrorKind if it fails
pub fn get_parents_from_nodehash(
    repo: Arc<BlobRepo>,
    node: HgNodeHash,
) -> impl Future<Item = Vec<HgChangesetId>, Error = Error> {
    repo.get_changeset_parents(&HgChangesetId::new(node))
        .map_err(|err| {
            ErrorKind::ParentsFetchFailed(BlobRepoErrorCause::new(
                err.downcast::<blobrepo::ErrorKind>().ok(),
            )).into()
        })
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use async_unit;
    use fixtures::linear;
    use futures::Future;
    use mononoke_types::Generation;

    use helpers::fetch_generation_and_join;
    use tests::string_to_nodehash;

    #[test]
    fn test_helpers() {
        async_unit::tokio_unit_test(move || {
            let repo = Arc::new(linear::getrepo(None));
            let mut ordered_hashes_oldest_to_newest = vec![
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                string_to_nodehash("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b"),
                string_to_nodehash("cb15ca4a43a59acff5388cea9648c162afde8372"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                string_to_nodehash("607314ef579bd2407752361ba1b0c1729d08b281"),
                string_to_nodehash("3e0e761030db6e479a7fb58b12881883f9f8c63f"),
                string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"),
            ];
            ordered_hashes_oldest_to_newest.reverse();

            for (i, node) in ordered_hashes_oldest_to_newest.into_iter().enumerate() {
                assert_eq!(
                    fetch_generation_and_join(repo.clone(), node)
                        .wait()
                        .unwrap(),
                    (node, Generation::new(i as u64 + 1))
                );
            }
        });
    }
}
