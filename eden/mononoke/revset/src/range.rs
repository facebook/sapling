/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_set::IntoIter;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter;
use std::mem::replace;

use anyhow::Error;
use cloned::cloned;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_ext::BoxStream;
use futures_ext::StreamExt;
use futures_old::future::Future;
use futures_old::stream;
use futures_old::stream::iter_ok;
use futures_old::stream::Stream;
use futures_old::Async;
use futures_old::Poll;

use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::errors::*;

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct HashGen {
    hash: ChangesetId,
    generation: Generation,
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ParentChild {
    parent: HashGen,
    child: HashGen,
}

pub struct RangeNodeStream {
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    start_node: ChangesetId,
    start_generation: Box<dyn Stream<Item = Generation, Error = Error> + Send>,
    children: HashMap<HashGen, HashSet<HashGen>>,
    // Child, parent
    pending_nodes: BoxStream<ParentChild, Error>,
    output_nodes: Option<BTreeMap<Generation, HashSet<ChangesetId>>>,
    drain: Option<IntoIter<ChangesetId>>,
}

fn make_pending(
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    child: HashGen,
) -> BoxStream<ParentChild, Error> {
    {
        cloned!(ctx, changeset_fetcher);
        let child_hash = child.hash;
        async move { changeset_fetcher.get_parents(ctx, child_hash).await }
    }
    .boxed()
    .compat()
    .map(move |parents| (child, parents))
    .map_err(|err| err.context(ErrorKind::ParentsFetchFailed))
    .map(|(child, parents)| iter_ok::<_, Error>(iter::repeat(child).zip(parents.into_iter())))
    .flatten_stream()
    .and_then(move |(child, parent_hash)| {
        {
            cloned!(ctx, changeset_fetcher, parent_hash);
            async move {
                changeset_fetcher
                    .get_generation_number(ctx, parent_hash)
                    .await
            }
        }
        .boxed()
        .compat()
        .map(move |gen_id| ParentChild {
            child,
            parent: HashGen {
                hash: parent_hash,
                generation: gen_id,
            },
        })
        .map_err(|err| err.context(ErrorKind::GenerationFetchFailed))
    })
    .boxify()
}

impl RangeNodeStream {
    // `start_node` should have a lower generation number than end_node,
    // otherwise stream will be empty
    pub fn new(
        ctx: CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
        start_node: ChangesetId,
        end_node: ChangesetId,
    ) -> Self {
        let start_generation = Box::new(
            {
                cloned!(ctx, changeset_fetcher);
                async move {
                    changeset_fetcher
                        .get_generation_number(ctx, start_node)
                        .await
                }
            }
            .boxed()
            .compat()
            .map_err(|err| err.context(ErrorKind::GenerationFetchFailed))
            .map(stream::repeat)
            .flatten_stream(),
        );

        let pending_nodes = {
            cloned!(ctx, changeset_fetcher);
            async move { changeset_fetcher.get_generation_number(ctx, end_node).await }
        }
        .boxed()
        .compat()
        .map_err(|err| err.context(ErrorKind::GenerationFetchFailed))
        .map({
            cloned!(ctx, changeset_fetcher);
            move |generation| {
                make_pending(
                    ctx,
                    changeset_fetcher,
                    HashGen {
                        hash: end_node,
                        generation,
                    },
                )
            }
        })
        .flatten_stream()
        .boxify();

        RangeNodeStream {
            ctx,
            changeset_fetcher,
            start_node,
            start_generation,
            children: HashMap::new(),
            pending_nodes,
            output_nodes: None,
            drain: None,
        }
    }

    fn build_output_nodes(&mut self, start_generation: Generation) {
        // We've been walking backwards from the end point, storing the nodes we see.
        // Now walk forward from the start point, looking at children only. These are
        // implicitly the range we want, because we only have children reachable from
        // the end point
        let mut output_nodes = BTreeMap::new();
        let mut nodes_to_handle = HashSet::new();
        // If this is empty, then the end node came before the start node
        if !self.children.is_empty() {
            nodes_to_handle.insert(HashGen {
                hash: self.start_node,
                generation: start_generation,
            });
        }
        while !nodes_to_handle.is_empty() {
            let nodes = std::mem::take(&mut nodes_to_handle);
            for hashgen in nodes {
                output_nodes
                    .entry(hashgen.generation)
                    .or_insert_with(HashSet::new)
                    .insert(hashgen.hash);
                if let Some(children) = self.children.get(&hashgen) {
                    nodes_to_handle.extend(children);
                }
            }
        }
        self.output_nodes = Some(output_nodes);
    }
}

impl Stream for RangeNodeStream {
    type Item = ChangesetId;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Empty the drain; this can only happen once we're in Stage 2
        let next_in_drain = self.drain.as_mut().and_then(|drain| drain.next());
        if next_in_drain.is_some() {
            return Ok(Async::Ready(next_in_drain));
        }

        // Stage 1 - no output nodes yet, so let's build it.
        if self.output_nodes.is_none() {
            loop {
                let start_generation = self.start_generation.poll()?;
                let start_generation = if let Async::Ready(Some(x)) = start_generation {
                    x
                } else {
                    return Ok(Async::NotReady);
                };

                let next_pending = self.pending_nodes.poll()?;
                let next_pending = match next_pending {
                    Async::Ready(None) => {
                        self.build_output_nodes(start_generation);
                        break;
                    }
                    Async::Ready(Some(x)) => x,
                    Async::NotReady => return Ok(Async::NotReady),
                };

                if next_pending.child.generation >= start_generation {
                    self.children
                        .entry(next_pending.parent)
                        .or_insert_with(HashSet::new)
                        .insert(next_pending.child);
                }

                if next_pending.parent.generation > start_generation {
                    let old_pending = replace(&mut self.pending_nodes, stream::empty().boxify());
                    let pending = old_pending.chain(make_pending(
                        self.ctx.clone(),
                        self.changeset_fetcher.clone(),
                        next_pending.parent,
                    ));
                    self.pending_nodes = pending.boxify();
                }
            }
        }

        // If we get here, we're in Stage 2 (and will never re-enter Stage 1).
        // Convert the tree of children that are ancestors of the end node into
        // the drain to output
        match self.output_nodes {
            Some(ref mut nodes) => {
                if !nodes.is_empty() {
                    let highest_generation =
                        *nodes.keys().max().expect("Non-empty map has no keys");
                    let current_generation = nodes
                        .remove(&highest_generation)
                        .expect("Highest generation doesn't exist");
                    self.drain = Some(current_generation.into_iter());
                    return Ok(Async::Ready(Some(
                        self.drain
                            .as_mut()
                            .and_then(|drain| drain.next())
                            .expect("Cannot create a generation without at least one node hash"),
                    )));
                }
            }
            _ => panic!("No output_nodes"),
        }
        Ok(Async::Ready(None))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fixtures::Linear;
    use crate::fixtures::MergeUneven;
    use crate::fixtures::TestRepoFixture;
    use blobrepo::BlobRepo;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures_ext::StreamExt;
    use mercurial_types::HgChangesetId;
    use revset_test_helper::assert_changesets_sequence;
    use revset_test_helper::string_to_nodehash;
    use std::sync::Arc;

    async fn string_to_bonsai<'a>(
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        s: &'static str,
    ) -> ChangesetId {
        let node = string_to_nodehash(s);
        repo.bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, HgChangesetId::new(node))
            .await
            .unwrap()
            .unwrap()
    }

    #[fbinit::test]
    async fn linear_range(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);

        let nodestream = RangeNodeStream::new(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
                string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
                string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
                string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
                string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn linear_direct_parent_range(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);

        let nodestream = RangeNodeStream::new(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
                string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn linear_single_node_range(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);

        let nodestream = RangeNodeStream::new(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn linear_empty_range(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);

        // These are swapped, so won't find anything
        let nodestream = RangeNodeStream::new(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
        )
        .boxify();

        assert_changesets_sequence(ctx.clone(), &repo, vec![], nodestream).await;
    }

    #[fbinit::test]
    async fn merge_range_from_merge(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(MergeUneven::getrepo(fb).await);

        let nodestream = RangeNodeStream::new(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
            string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await,
                string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
                string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn merge_range_everything(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(MergeUneven::getrepo(fb).await);

        let nodestream = RangeNodeStream::new(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await,
            string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await,
                string_to_bonsai(&ctx, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
                string_to_bonsai(&ctx, &repo, "5d43888a3c972fe68c224f93d41b30e9f888df7c").await,
                string_to_bonsai(&ctx, &repo, "fc2cef43395ff3a7b28159007f63d6529d2f41ca").await,
                string_to_bonsai(&ctx, &repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33").await,
                string_to_bonsai(&ctx, &repo, "795b8133cf375f6d68d27c6c23db24cd5d0cd00f").await,
                string_to_bonsai(&ctx, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
                string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
                string_to_bonsai(&ctx, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
                string_to_bonsai(&ctx, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
                string_to_bonsai(&ctx, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
                string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await,
            ],
            nodestream,
        )
        .await;
    }
}
