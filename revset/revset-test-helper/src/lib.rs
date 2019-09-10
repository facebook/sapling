// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use context::CoreContext;
use failure_ext::Error;
use futures::executor::spawn;
use futures::future::Future;
use futures::stream::Stream;
use futures_ext::BoxStream;
use mercurial_types::nodehash::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::ChangesetId;

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

pub fn single_changeset_id(
    ctx: CoreContext,
    cs_id: ChangesetId,
    repo: &BlobRepo,
) -> impl Stream<Item = ChangesetId, Error = Error> {
    repo.changeset_exists_by_bonsai(ctx, cs_id)
        .map(move |exists| if exists { Some(cs_id) } else { None })
        .into_stream()
        .filter_map(|maybenode| maybenode)
}

pub fn string_to_nodehash(hash: &str) -> HgNodeHash {
    HgNodeHash::from_str(hash).expect("Can't turn string to HgNodeHash")
}

pub fn string_to_bonsai(repo: &Arc<BlobRepo>, s: &str) -> ChangesetId {
    let ctx = CoreContext::test_mock();
    let node = string_to_nodehash(s);
    repo.get_bonsai_from_hg(ctx, HgChangesetId::new(node))
        .wait()
        .unwrap()
        .unwrap()
}

pub fn assert_changesets_sequence<I>(
    ctx: CoreContext,
    repo: &Arc<BlobRepo>,
    hashes: I,
    stream: BoxStream<ChangesetId, Error>,
) where
    I: IntoIterator<Item = ChangesetId>,
{
    let mut nodestream = spawn(stream);
    let mut received_hashes = HashSet::new();
    for expected in hashes {
        // If we pulled it in earlier, we've found it.
        if received_hashes.remove(&expected) {
            continue;
        }

        let expected_generation = repo
            .clone()
            .get_generation_number_by_bonsai(ctx.clone(), expected)
            .wait()
            .expect("Unexpected error");

        // Keep pulling in hashes until we either find this one, or move on to a new generation
        loop {
            let hash = nodestream
                .wait_stream()
                .expect("Unexpected end of stream")
                .expect("Unexpected error");

            if hash == expected {
                break;
            }

            let node_generation = repo
                .clone()
                .get_generation_number_by_bonsai(ctx.clone(), expected)
                .wait()
                .expect("Unexpected error");

            assert!(
                node_generation == expected_generation,
                "Did not receive expected node {:?} before change of generation from {:?} to {:?}",
                expected,
                node_generation,
                expected_generation,
            );

            received_hashes.insert(hash);
        }
    }

    assert!(
        received_hashes.is_empty(),
        "Too few nodes received: {:?}",
        received_hashes
    );

    let next_node = nodestream.wait_stream();
    assert!(
        next_node.is_none(),
        "Too many nodes received: {:?}",
        next_node.unwrap()
    );
}

#[cfg(test)]
mod test {
    use super::*;
    use context::CoreContext;
    use fixtures::linear;
    use futures_ext::StreamExt;
    use mononoke_types_mocks::changesetid::ONES_CSID;

    #[test]
    fn valid_changeset() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let bcs_id = string_to_bonsai(&repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let changeset_stream = single_changeset_id(ctx.clone(), bcs_id.clone(), &repo);

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![bcs_id].into_iter(),
                changeset_stream.boxify(),
            );
        });
    }

    #[test]
    fn invalid_changeset() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let cs_id = ONES_CSID;
            let changeset_stream = single_changeset_id(ctx.clone(), cs_id, &repo.clone());

            assert_changesets_sequence(ctx, &repo, vec![].into_iter(), changeset_stream.boxify());
        });
    }
}
