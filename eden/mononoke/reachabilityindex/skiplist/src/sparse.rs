/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use slog::debug;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::num::NonZeroU64;

use crate::NodeFrontier;
use crate::SkiplistNodeType;
use changeset_fetcher::ArcChangesetFetcher;
use common::fetch_generations;
use common::fetch_parents_and_generations;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

/// Update skiplist index so that all heads are indexed.
///
/// Note that this function doesn't add every new changeset to the index.
/// Rather this function does the following:
/// 1) Adds all heads to the index
/// 2) Each indexed changeset points to another indexed changeset
/// 3) Each edge skips at most `max_skip` changesets
/// 4) It tries to keep a majority of changesets unidexed
///
/// ```text
/// For example
///
///  max_skip = 3
///
///  A <- head is indexed, points to D
///  |
///  B <- not indexed
///  |
///  |  E <- head is indexed, points to D. Note - this edge skips only 2 commits
///  | /
///  C <- unindexed
///  |
///  D <- indexed, points to ancestors of E
///  |
///  E <- not indexed
///  |
/// ...
///
///
///
/// This skiplist structure allows both quick traversal of the graph from an
/// indexed node, and also it makes sure that from unindexed node one don't need to
/// visit more than max_skip ancestors to find an indexed node.
/// ```
pub async fn update_sparse_skiplist(
    ctx: &CoreContext,
    heads: Vec<ChangesetId>,
    index: &mut HashMap<ChangesetId, SkiplistNodeType>,
    max_skip: NonZeroU64,
    cs_fetcher: &ArcChangesetFetcher,
) -> Result<(), Error> {
    let heads_with_gens = fetch_generations(ctx, cs_fetcher, heads.clone()).await?;

    let mut node_frontier = NodeFrontier::from_iter(heads_with_gens);
    // We start indexing from the changesets with the largest generation numbers.
    // The motivation is to make sure that the ancestors of the main branch skip
    // as many commits as possible. It's unclear how much it matters in practice
    // but at least it shouldn't be worse than other ordering.
    while let Some((gen, cs_ids)) = node_frontier.remove_max_gen() {
        for cs_id in cs_ids {
            let new_cs_ids =
                index_changeset(ctx, (cs_id, gen), index, max_skip, cs_fetcher).await?;
            node_frontier.extend(new_cs_ids);
        }
    }

    // After we've indexed new nodes we might have nodes that are not reachable
    // by any head. These nodes we'd like to delete.
    debug!(ctx.logger(), "trimming, current size: {}", index.len());
    remove_unreachable_nodes(heads, index);
    debug!(ctx.logger(), "trimmed, current size: {}", index.len());
    Ok(())
}

/// Traverses ancestors of `cs_id` until one of the following is true:
/// 1) an already indexed node is found
/// 2) a merge commit is found
/// 3) reached the end of the graph
///
/// While traversing the graph new nodes are added to the index if there's >= max_skip
/// commits between them
/// If this function found a merge then parents of this merge commits are returned,
/// and they should be indexed later.
async fn index_changeset(
    ctx: &CoreContext,
    mut edge_start: (ChangesetId, Generation),
    index: &mut HashMap<ChangesetId, SkiplistNodeType>,
    max_skip: NonZeroU64,
    cs_fetcher: &ArcChangesetFetcher,
) -> Result<Vec<(ChangesetId, Generation)>, Error> {
    if index.contains_key(&edge_start.0) {
        return Ok(vec![]);
    }
    let internal_err_msg = "programming error: invalid gen number";

    let mut edge_end = edge_start;
    // edge_start will be the start of the new skiplist edge that we'll insert into the index,
    // and edge_end will the end of this edge. The algorithm works as follows:
    // In a loop:
    //   Traverse the parents of edge_end. If a merge commit or an already
    //   indexed commit found then just insert the edge and exit the loop.
    //   Otherwise if edge_end is more than max_skip commits from edge_start
    //   then insert the edge to the skiplist, move edge_start to edge_end and
    //   continue the loop.
    loop {
        // Found already indexed node - insert the edge and exit
        if let Some(mut edge) = index.get(&edge_end.0).cloned() {
            let mut new_edge_end = None;

            // The indexed node that we just found might be too close to us.
            // Let's see how many changesets an edge from the node we just
            // found skips. If distance from edge_start to this node plus
            // the length of the edge is smaller than max_skip then we can increase
            // then length of the edge.
            while let SkiplistNodeType::SingleEdge((next_cs, next_gen)) = edge {
                // Find how far away edge_start is from the end of the edge...
                let difference = edge_start
                    .1
                    .difference_from(next_gen)
                    .ok_or_else(|| anyhow!(internal_err_msg))?;
                if difference <= max_skip.get() {
                    // ...not too far! Let's instead point edge_start to edge_end
                    new_edge_end = Some((next_cs, next_gen));
                    if let Some(next_edge) = index.get(&next_cs) {
                        edge = next_edge.clone();
                        continue;
                    }
                }
                break;
            }

            index.insert(
                edge_start.0,
                SkiplistNodeType::SingleEdge(new_edge_end.unwrap_or(edge_end)),
            );

            return Ok(vec![]);
        }

        if edge_start != edge_end {
            let difference = edge_start
                .1
                .difference_from(edge_end.1)
                .ok_or_else(|| anyhow!(internal_err_msg))?;

            if difference >= max_skip.get() {
                let edge = SkiplistNodeType::SingleEdge(edge_end);
                index.insert(edge_start.0, edge);
                edge_start = edge_end;
            }
        }

        let parents = fetch_parents_and_generations(ctx, cs_fetcher, edge_end.0).await?;
        match parents.as_slice() {
            [] => {
                return Ok(vec![]);
            }
            [p1] => {
                edge_end = *p1;
            }
            _ => {
                if edge_start != edge_end {
                    let edge = SkiplistNodeType::SingleEdge(edge_end);
                    index.insert(edge_start.0, edge);
                }
                index.insert(edge_end.0, SkiplistNodeType::ParentEdges(parents.clone()));

                return Ok(parents);
            }
        };
    }
}

// Leaves only nodes that are reachable from heads
fn remove_unreachable_nodes(
    heads: Vec<ChangesetId>,
    index: &mut HashMap<ChangesetId, SkiplistNodeType>,
) {
    let mut visited: HashSet<_> = heads.into_iter().collect();
    let mut q: VecDeque<_> = visited.clone().into_iter().collect();

    while let Some(cs_id) = q.pop_back() {
        use SkiplistNodeType::*;
        let children = match index.get(&cs_id) {
            Some(SingleEdge((cs_id, _))) => vec![*cs_id],
            Some(SkipEdges(edges)) | Some(ParentEdges(edges)) => {
                edges.iter().cloned().map(|(cs_id, _)| cs_id).collect()
            }
            None => vec![],
        };
        q.extend(children.into_iter().filter(|c| visited.insert(*c)));
    }

    // Now keep in skiplist only nodes that are reachable from heads
    index.retain(|k, _| visited.contains(k));
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::BlobRepo;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use std::collections::VecDeque;
    use tests_utils::drawdag::create_from_dag;
    use tests_utils::resolve_cs_id;

    #[fbinit::test]
    async fn test_index_changeset_linear(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;

        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let master_gen_num = cs_fetcher
            .get_generation_number(ctx.clone(), master_cs_id)
            .await?;

        let max_skip = NonZeroU64::new(2).unwrap();
        index_changeset(
            &ctx,
            (master_cs_id, master_gen_num),
            &mut index,
            max_skip,
            &cs_fetcher,
        )
        .await?;

        // 11 commits total
        assert_eq!(index.len(), 5);
        validate_index(master_cs_id, &index, max_skip).await?;

        let mut index = HashMap::new();
        let max_skip = NonZeroU64::new(3).unwrap();
        index_changeset(
            &ctx,
            (master_cs_id, master_gen_num),
            &mut index,
            max_skip,
            &cs_fetcher,
        )
        .await?;

        // 11 commits total
        assert_eq!(index.len(), 3);
        validate_index(master_cs_id, &index, max_skip).await?;

        let max_skip = NonZeroU64::new(1).unwrap();
        let mut index = HashMap::new();
        index_changeset(
            &ctx,
            (master_cs_id, master_gen_num),
            &mut index,
            max_skip,
            &cs_fetcher,
        )
        .await?;

        // 11 commits total
        assert_eq!(index.len(), 10);
        validate_index(master_cs_id, &index, max_skip).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_index_changeset_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F
                /
               B
             "##,
        )
        .await?;

        // These are three changesets that will have entries in index
        let c = *dag.get("C").unwrap();
        let d = *dag.get("D").unwrap();
        let f = *dag.get("F").unwrap();
        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let f_gen_num = cs_fetcher.get_generation_number(ctx.clone(), f).await?;

        let max_skip = NonZeroU64::new(2).unwrap();
        index_changeset(&ctx, (f, f_gen_num), &mut index, max_skip, &cs_fetcher).await?;

        assert_eq!(index.len(), 3);
        validate_index(f, &index, max_skip).await?;

        assert!(index.contains_key(&c));
        assert!(index.contains_key(&d));
        assert!(index.contains_key(&f));

        Ok(())
    }

    #[fbinit::test]
    async fn test_index_cleanup(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F-G-H-I-J-K
             "##,
        )
        .await?;

        // These are three changesets that will have entries in index
        let g = *dag.get("G").unwrap();

        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let g_gen_num = cs_fetcher.get_generation_number(ctx.clone(), g).await?;

        let max_skip = NonZeroU64::new(4).unwrap();
        index_changeset(&ctx, (g, g_gen_num), &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 1);
        validate_index(g, &index, max_skip).await?;

        for vert in &["H", "I", "J", "K"] {
            println!("vertex {}", vert);
            let vert = *dag.get(*vert).unwrap();
            update_sparse_skiplist(&ctx, vec![vert], &mut index, max_skip, &cs_fetcher).await?;
            assert_eq!(index.len(), 2);
            validate_index(vert, &index, max_skip).await?;
        }

        Ok(())
    }

    #[fbinit::test]
    async fn test_index_cleanup_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F-G
                /
               H
             "##,
        )
        .await?;

        // These are three changesets that will have entries in index
        let d = *dag.get("D").unwrap();

        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let d_gen_num = cs_fetcher.get_generation_number(ctx.clone(), d).await?;

        let max_skip = NonZeroU64::new(4).unwrap();
        index_changeset(&ctx, (d, d_gen_num), &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 2);
        validate_index(d, &index, max_skip).await?;

        for vert in &["E", "F", "G"] {
            println!("vertex {}", vert);
            let vert = *dag.get(*vert).unwrap();
            update_sparse_skiplist(&ctx, vec![vert], &mut index, max_skip, &cs_fetcher).await?;
            assert_eq!(index.len(), 2);
            validate_index(vert, &index, max_skip).await?;
        }

        // These nodes should be in the graph
        let c = *dag.get("C").unwrap();
        assert!(index.contains_key(&c));
        let g = *dag.get("G").unwrap();
        assert!(index.contains_key(&g));

        Ok(())
    }

    #[fbinit::test]
    async fn test_skiplist_deletions_linear(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F-G-H-I-J-K-L
             "##,
        )
        .await?;

        // These are three changesets that will have entries in index
        let g = *dag.get("G").unwrap();
        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let g_gen_num = cs_fetcher.get_generation_number(ctx.clone(), g).await?;

        let max_skip = NonZeroU64::new(5).unwrap();
        index_changeset(&ctx, (g, g_gen_num), &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 1);
        validate_index(g, &index, max_skip).await?;
        assert!(index.contains_key(&g));

        for i in &["H", "I", "J", "K", "L"] {
            let i = *dag.get(*i).unwrap();
            let i_gen_num = cs_fetcher.get_generation_number(ctx.clone(), i).await?;

            index_changeset(&ctx, (i, i_gen_num), &mut index, max_skip, &cs_fetcher).await?;
            validate_index(i, &index, max_skip).await?;
            assert!(index.contains_key(&i));
        }

        let g = *dag.get("G").unwrap();
        assert!(index.contains_key(&g));
        let l = *dag.get("L").unwrap();
        assert!(index.contains_key(&l));

        Ok(())
    }

    #[fbinit::test]
    async fn test_build_skiplist_heads(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
                             M
                            /
               A-C-D-E-F-G-H-I-J-K-L
                              \
                               N
             "##,
        )
        .await?;

        let g = *dag.get("G").unwrap();
        let l = *dag.get("L").unwrap();
        let m = *dag.get("M").unwrap();
        let n = *dag.get("N").unwrap();
        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let max_skip = NonZeroU64::new(5).unwrap();

        update_sparse_skiplist(&ctx, vec![l, m, n], &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 4);
        assert!(index.contains_key(&g));
        assert!(index.contains_key(&l));
        assert!(index.contains_key(&m));
        assert!(index.contains_key(&n));
        validate_index(g, &index, max_skip).await?;
        validate_index(l, &index, max_skip).await?;
        validate_index(m, &index, max_skip).await?;
        validate_index(n, &index, max_skip).await?;

        match index.get(&m) {
            Some(SkiplistNodeType::SingleEdge(edge)) => {
                assert_eq!(edge.0, g);
            }
            _ => {
                panic!("unexpected edge from M");
            }
        }

        match index.get(&n) {
            Some(SkiplistNodeType::SingleEdge(edge)) => {
                assert_eq!(edge.0, g);
            }
            _ => {
                panic!("unexpected edge from N");
            }
        }
        Ok(())
    }

    #[fbinit::test]
    async fn test_build_skiplist_heads_ancestors_of_each_other(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F-G-H-I-J-K-L
             "##,
        )
        .await?;

        // These are three changesets that will have entries in index
        let g = *dag.get("G").unwrap();
        let h = *dag.get("H").unwrap();
        let l = *dag.get("L").unwrap();
        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let max_skip = NonZeroU64::new(5).unwrap();

        // Build index with two heads - "L" and "H". We should have a three skiplist edges:
        // L -> G, G->A and H->G.
        update_sparse_skiplist(&ctx, vec![h, l], &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 3);
        assert!(index.contains_key(&g));
        assert!(index.contains_key(&h));
        assert!(index.contains_key(&l));
        validate_index(l, &index, max_skip).await?;
        validate_index(h, &index, max_skip).await?;
        match index.get(&h) {
            Some(SkiplistNodeType::SingleEdge(edge)) => {
                assert_eq!(edge.0, g);
            }
            _ => {
                panic!("unexpected edge from H");
            }
        }

        Ok(())
    }

    #[fbinit::test]
    async fn test_skiplist_deletions_with_merges(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F
                /
               B
             "##,
        )
        .await?;

        // These are three changesets that will have entries in index
        let c = *dag.get("C").unwrap();
        let d = *dag.get("D").unwrap();
        let e = *dag.get("E").unwrap();
        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let d_gen_num = cs_fetcher.get_generation_number(ctx.clone(), d).await?;

        let max_skip = NonZeroU64::new(2).unwrap();
        index_changeset(&ctx, (d, d_gen_num), &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 2);
        validate_index(d, &index, max_skip).await?;
        assert!(index.contains_key(&c));
        assert!(index.contains_key(&d));

        let e_gen_num = cs_fetcher.get_generation_number(ctx.clone(), e).await?;
        index_changeset(&ctx, (e, e_gen_num), &mut index, max_skip, &cs_fetcher).await?;
        assert_eq!(index.len(), 3);
        validate_index(e, &index, max_skip).await?;
        assert!(index.contains_key(&c));
        assert!(index.contains_key(&e));

        Ok(())
    }

    #[fbinit::test]
    async fn test_build_skiplist(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let dag = create_from_dag(
            &ctx,
            &repo,
            r##"
               A-C-D-E-F
                    /
                   B
             "##,
        )
        .await?;

        let cs_fetcher = repo.get_changeset_fetcher();
        let mut index = HashMap::new();

        let max_skip = NonZeroU64::new(2).unwrap();
        let d = *dag.get("D").unwrap();
        let e = *dag.get("E").unwrap();
        let f = *dag.get("F").unwrap();
        update_sparse_skiplist(&ctx, vec![f], &mut index, max_skip, &cs_fetcher).await?;

        assert_eq!(index.len(), 3);
        validate_index(f, &index, max_skip).await?;
        assert!(index.contains_key(&d));
        assert!(index.contains_key(&e));
        assert!(index.contains_key(&f));

        Ok(())
    }

    async fn validate_index(
        start: ChangesetId,
        index: &HashMap<ChangesetId, SkiplistNodeType>,
        max_skip: NonZeroU64,
    ) -> Result<(), Error> {
        let mut queue = VecDeque::new();
        queue.push_back(start);
        while let Some(cur) = queue.pop_front() {
            match index.get(&cur) {
                Some(edge) => {
                    use SkiplistNodeType::*;
                    let edges = match edge {
                        SingleEdge(next) => vec![*next],
                        ParentEdges(edges) => edges.clone(),
                        SkipEdges(edges) => edges.clone(),
                    };

                    for edge in edges {
                        if edge.1.value() > max_skip.get() {
                            queue.push_back(edge.0);
                        }
                    }
                }
                None => {
                    return Err(anyhow!("{} should be in the index", cur));
                }
            }
        }
        Ok(())
    }
}
