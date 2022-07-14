/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Stream01CompatExt;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures_ext::BoxStream;
use futures_old::future::Future;
use futures_old::stream::Stream;
use mercurial_types::nodehash::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::ChangesetId;

use std::collections::HashSet;
use std::str::FromStr;

pub fn single_changeset_id(
    ctx: CoreContext,
    cs_id: ChangesetId,
    repo: &BlobRepo,
) -> impl Stream<Item = ChangesetId, Error = Error> {
    let repo = repo.clone();
    async move { repo.changeset_exists_by_bonsai(ctx, cs_id).await }
        .boxed()
        .compat()
        .map(move |exists| if exists { Some(cs_id) } else { None })
        .into_stream()
        .filter_map(|maybenode| maybenode)
}

pub fn string_to_nodehash(hash: &str) -> HgNodeHash {
    HgNodeHash::from_str(hash).expect("Can't turn string to HgNodeHash")
}

pub async fn string_to_bonsai(fb: FacebookInit, repo: &BlobRepo, s: &str) -> ChangesetId {
    let ctx = CoreContext::test_mock(fb);
    let node = string_to_nodehash(s);
    repo.bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, HgChangesetId::new(node))
        .await
        .unwrap()
        .unwrap()
}

pub async fn assert_changesets_sequence<I>(
    ctx: CoreContext,
    repo: &BlobRepo,
    hashes: I,
    stream: BoxStream<ChangesetId, Error>,
) where
    I: IntoIterator<Item = ChangesetId>,
{
    let mut nodestream = stream.compat();
    let mut received_hashes = HashSet::new();
    for expected in hashes {
        // If we pulled it in earlier, we've found it.
        if received_hashes.remove(&expected) {
            continue;
        }

        let expected_generation = repo
            .clone()
            .get_generation_number(ctx.clone(), expected)
            .await
            .expect("Unexpected error");

        // Keep pulling in hashes until we either find this one, or move on to a new generation
        loop {
            let hash = nodestream
                .next()
                .await
                .expect("Unexpected end of stream")
                .expect("Unexpected error");

            if hash == expected {
                break;
            }

            let node_generation = repo
                .clone()
                .get_generation_number(ctx.clone(), expected)
                .await
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

    let next_node = nodestream.next().await;
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
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures_ext::StreamExt;
    use mononoke_types_mocks::changesetid::ONES_CSID;

    #[fbinit::test]
    async fn valid_changeset(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let bcs_id = string_to_bonsai(fb, &repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a").await;
        let changeset_stream = single_changeset_id(ctx.clone(), bcs_id.clone(), &repo);

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![bcs_id].into_iter(),
            changeset_stream.boxify(),
        )
        .await;
    }

    #[fbinit::test]
    async fn invalid_changeset(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let cs_id = ONES_CSID;
        let changeset_stream = single_changeset_id(ctx.clone(), cs_id, &repo.clone());

        assert_changesets_sequence(ctx, &repo, vec![].into_iter(), changeset_stream.boxify()).await;
    }
}
