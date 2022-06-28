/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::time::Duration;

use anyhow::Error;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream::TryStreamExt;
use pretty_assertions::assert_eq;
use quickcheck::empty_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use tokio::task::yield_now;

use super::utils::StateLog;
use super::utils::Tick;
use crate::bounded_traversal_limited_ordered_stream;
use crate::bounded_traversal_ordered_stream;
use crate::OrderedTraversal;

/// Ordered tree for test purposes
#[derive(Clone, Debug)]
enum OrdTree {
    /// A leaf is an item to yield.
    Leaf(usize),

    /// A node is a sequence of subtrees.  Nodes have identities for logging
    /// purposes, but these are not included in the yielded output.
    Node(usize, Vec<OrdTree>),
}

impl OrdTree {
    fn size(&self) -> usize {
        match self {
            OrdTree::Leaf(_) => 1,
            OrdTree::Node(_, children) => children.iter().map(OrdTree::size).sum(),
        }
    }

    fn make_arbitrary(g: &mut Gen, size: usize) -> Self {
        if size <= 1 {
            OrdTree::Leaf(usize::arbitrary(g))
        } else {
            let id = usize::arbitrary(g);
            let child_count = 1 + usize::arbitrary(g) % size;
            let mut children = Vec::new();
            let mut spare_size = size - child_count;
            for _ in 0..=child_count {
                let child_size = usize::arbitrary(g) % (spare_size + 1);
                spare_size -= child_size;
                children.push(OrdTree::make_arbitrary(g, child_size + 1));
            }
            children.push(OrdTree::make_arbitrary(g, spare_size + 1));
            // shuffle the children using the Fisher–Yates algorithm
            let n = children.len();
            for i in 0..(n - 1) {
                // j <- random index such that i <= j < n
                let j = i + usize::arbitrary(g) % (n - i);
                children.swap(i, j);
            }
            OrdTree::Node(id, children)
        }
    }

    fn visit(&self, visitor: &mut Vec<usize>) {
        match self {
            OrdTree::Leaf(v) => visitor.push(*v),
            OrdTree::Node(_, children) => {
                for c in children {
                    c.visit(visitor);
                }
            }
        }
    }
}

impl Arbitrary for OrdTree {
    fn arbitrary(g: &mut Gen) -> Self {
        let size = g.size();
        OrdTree::make_arbitrary(g, size)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        match self {
            OrdTree::Leaf(_) => empty_shrinker(),
            OrdTree::Node(id, children) => {
                let id = *id;
                Box::new(children.shrink().map(move |c| OrdTree::Node(id, c)))
            }
        }
    }
}

fn make_ordered_tree() -> OrdTree {
    // tree
    //                           100
    //                            |
    //     -----------------------------------------------------
    //     110          (6)          130         (12)        150
    //      |                         |                       |
    // ------------------   ----------------------    -----------------
    // (0)  112  (4)  114   131      132      (11)    151    152    153
    //       |         |     |        |                |      |      |
    //  -----------   ---   ---  ------------         ----   ----   ----
    //  (1) (2) (3)   (5)   (7)  (8) (9) (10)         (13)   (14)   (15)
    //
    OrdTree::Node(
        100,
        vec![
            OrdTree::Node(
                110,
                vec![
                    OrdTree::Leaf(0),
                    OrdTree::Node(
                        112,
                        vec![OrdTree::Leaf(1), OrdTree::Leaf(2), OrdTree::Leaf(3)],
                    ),
                    OrdTree::Leaf(4),
                    OrdTree::Node(114, vec![OrdTree::Leaf(5)]),
                ],
            ),
            OrdTree::Leaf(6),
            OrdTree::Node(
                130,
                vec![
                    OrdTree::Node(131, vec![OrdTree::Leaf(7)]),
                    OrdTree::Node(
                        132,
                        vec![OrdTree::Leaf(8), OrdTree::Leaf(9), OrdTree::Leaf(10)],
                    ),
                    OrdTree::Leaf(11),
                ],
            ),
            OrdTree::Leaf(12),
            OrdTree::Node(
                150,
                vec![
                    OrdTree::Node(151, vec![OrdTree::Leaf(13)]),
                    OrdTree::Node(152, vec![OrdTree::Leaf(14)]),
                    OrdTree::Node(153, vec![OrdTree::Leaf(15)]),
                ],
            ),
        ],
    )
}

#[tokio::test]
async fn test_bounded_traversal_ordered_stream() -> Result<(), Error> {
    let tree = make_ordered_tree();
    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();

    let traverse = {
        let tick = tick.clone();
        let log = log.clone();
        bounded_traversal_ordered_stream(
            // schedule_max
            NonZeroUsize::new(2).unwrap(),
            // queue_max
            NonZeroUsize::new(3).unwrap(),
            // init
            Some((tree.size(), tree)),
            // unfold
            {
                move |tree| {
                    let log = log.clone();
                    match tree {
                        OrdTree::Node(id, children) => {
                            let sleep = tick.sleep(1);
                            async move {
                                let now = sleep.await;
                                log.unfold(id, now);

                                Ok::<_, Error>(children.into_iter().map(|child| match child {
                                    OrdTree::Leaf(id) => OrderedTraversal::Output(id),
                                    subtree => OrderedTraversal::Recurse(subtree.size(), subtree),
                                }))
                            }
                            .boxed()
                        }
                        OrdTree::Leaf(out) => {
                            panic!("unfold called on leaf {}", out)
                        }
                    }
                }
            },
        )
        .try_collect::<Vec<usize>>()
        .boxed()
    };
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(100, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(110, 2);
    // Node 130 is not unfolded as there is insufficient queue budget for it
    // while we are unfolding the tree rooted at node 110.
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(112, 3);
    // Nodes 114 and 130 are not unfolded as there is still insufficient
    // queue budget for them.
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(114, 4);
    // We've unfolded and yielded from the tree rooted at 110 enough that
    // there is now sufficient queue budget to start the tree rooted at 130.
    reference.unfold(130, 4);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(131, 5);
    reference.unfold(132, 5);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(150, 6);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(151, 7);
    reference.unfold(152, 7);
    // We don't start unfolding the tree rooted at 153 even though there is
    // sufficient queue budget, as we have reached the `scheduled_max` limit.
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(153, 8);
    assert_eq!(log, reference);

    assert_eq!(handle.await??, (0..16).collect::<Vec<_>>());

    Ok(())
}

#[tokio::test]
async fn test_bounded_traversal_limited_ordered_stream() -> Result<(), Error> {
    let tree = make_ordered_tree();
    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();

    let traverse = {
        let tick = tick.clone();
        let log = log.clone();
        bounded_traversal_limited_ordered_stream(
            // schedule_max
            NonZeroUsize::new(2).unwrap(),
            // queue_max
            NonZeroUsize::new(3).unwrap(),
            // limit
            8,
            // init
            Some((tree.size(), tree)),
            // unfold
            {
                move |tree| {
                    let log = log.clone();
                    match tree {
                        OrdTree::Node(id, children) => {
                            let sleep = tick.sleep(1);
                            async move {
                                let now = sleep.await;
                                log.unfold(id, now);

                                Ok::<_, Error>(children.into_iter().map(|child| match child {
                                    OrdTree::Leaf(id) => OrderedTraversal::Output(id),
                                    subtree => OrderedTraversal::Recurse(subtree.size(), subtree),
                                }))
                            }
                            .boxed()
                        }
                        OrdTree::Leaf(out) => {
                            panic!("unfold called on leaf {}", out)
                        }
                    }
                }
            },
        )
        .try_collect::<Vec<usize>>()
        .boxed()
    };
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(100, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(110, 2);
    // Node 130 is not unfolded as there is insufficient queue budget for it
    // while we are unfolding the tree rooted at node 110.
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(112, 3);
    // Nodes 114 and 130 are not unfolded as there is still insufficient
    // queue budget for them.
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(114, 4);
    // We've unfolded and yielded from the tree rooted at 110 enough that
    // there is now sufficient queue budget to start the tree rooted at 130.
    reference.unfold(130, 4);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(131, 5);
    // Unfolding and yielding stops here as we have reached the limit.
    assert_eq!(log, reference);

    assert_eq!(handle.await??, (0..8).collect::<Vec<_>>());

    Ok(())
}

#[tokio::test]
async fn test_bounded_traversal_limited_ordered_stream_partial() -> Result<(), Error> {
    let tree = make_ordered_tree();
    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();

    // Traverse the tree.  In this test, odd leaves less than ten are not yielded, and the
    // whole subtrees of nodes 112 and 130 are omitted.
    let traverse = {
        let tick = tick.clone();
        let log = log.clone();
        bounded_traversal_limited_ordered_stream(
            // schedule_max
            NonZeroUsize::new(2).unwrap(),
            // queue_max
            NonZeroUsize::new(3).unwrap(),
            // limit
            5,
            // init
            Some((tree.size(), tree)),
            // unfold
            {
                move |tree| {
                    let log = log.clone();
                    match tree {
                        OrdTree::Node(id, children) => {
                            let sleep = tick.sleep(1);
                            async move {
                                let now = sleep.await;
                                log.unfold(id, now);

                                Ok::<_, Error>(children.into_iter().filter_map(
                                    |child| match child {
                                        OrdTree::Leaf(id) if id % 2 == 1 && id < 10 => None,
                                        OrdTree::Node(id, _) if id == 112 => None,
                                        OrdTree::Node(id, _) if id == 130 => None,
                                        OrdTree::Leaf(id) => Some(OrderedTraversal::Output(id)),
                                        subtree => {
                                            Some(OrderedTraversal::Recurse(subtree.size(), subtree))
                                        }
                                    },
                                ))
                            }
                            .boxed()
                        }
                        OrdTree::Leaf(out) => {
                            panic!("unfold called on leaf {}", out)
                        }
                    }
                }
            },
        )
        .try_collect::<Vec<usize>>()
        .boxed()
    };
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(100, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(110, 2);
    // Node 130 is not unfolded as it is excluded.  Node 150 is not unfolded
    // as there is insufficient queue budget for it while we are unfolding the
    // tree rooted at node 110.
    assert_eq!(log, reference);

    tick.tick().await;
    // Node 112 is not unfolded as it is excluded.
    reference.unfold(114, 3);
    assert_eq!(log, reference);

    tick.tick().await;
    // There is now sufficient queue budget to start unfolding node 150.
    reference.unfold(150, 4);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(151, 5);
    // Nodes 152 and 153 are not unfolded as we have reached the limit.
    assert_eq!(log, reference);

    assert_eq!(handle.await??, vec![0, 4, 6, 12, 13]);

    Ok(())
}

fn quickcheck_unfold(
    tree: OrdTree,
) -> BoxFuture<'static, Result<impl IntoIterator<Item = OrderedTraversal<usize, OrdTree>>, Error>> {
    match tree {
        OrdTree::Node(id, children) => async move {
            if id % 10 > 0 {
                tokio::time::sleep(Duration::from_micros((id % 10) as u64)).await;
            }
            Ok(children.into_iter().map(|child| match child {
                OrdTree::Leaf(id) => OrderedTraversal::Output(id),
                subtree => OrderedTraversal::Recurse(subtree.size(), subtree),
            }))
        }
        .boxed(),
        OrdTree::Leaf(out) => {
            panic!("unfold called on leaf {}", out)
        }
    }
}

#[quickcheck_async::tokio]
async fn quickcheck_bounded_traversal_ordered_stream(
    tree: OrdTree,
    schedule_max: NonZeroUsize,
    queue_max: NonZeroUsize,
) -> bool {
    let mut order = Vec::new();
    tree.visit(&mut order);
    let streamed = bounded_traversal_ordered_stream(
        schedule_max,
        queue_max,
        Some((tree.size(), tree)),
        quickcheck_unfold,
    )
    .try_collect::<Vec<usize>>()
    .await
    .unwrap();

    streamed == order
}

#[quickcheck_async::tokio]
async fn quickcheck_bounded_traversal_limited_ordered_stream(
    tree: OrdTree,
    schedule_max: NonZeroUsize,
    queue_max: NonZeroUsize,
    limit: usize,
) -> bool {
    let mut order = Vec::new();
    tree.visit(&mut order);
    order.truncate(limit);
    let streamed = bounded_traversal_limited_ordered_stream(
        schedule_max,
        queue_max,
        limit,
        Some((tree.size(), tree)),
        quickcheck_unfold,
    )
    .try_collect::<Vec<usize>>()
    .await
    .unwrap();

    streamed == order
}
