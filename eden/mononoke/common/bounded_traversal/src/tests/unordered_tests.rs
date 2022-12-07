/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use anyhow::Error;
use cloned::cloned;
use futures::future::FutureExt;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use maplit::hashmap;
use pretty_assertions::assert_eq;
use tokio::task::yield_now;

use super::utils::StateLog;
use super::utils::Tick;
use crate::bounded_traversal;
use crate::bounded_traversal_dag;
use crate::bounded_traversal_stream;
use crate::limited_by_key_shardable;

// Tree for test purposes
#[derive(Debug)]
struct Tree {
    id: usize,
    children: Vec<Tree>,
}

impl Tree {
    fn new(id: usize, children: Vec<Tree>) -> Self {
        Self { id, children }
    }

    fn leaf(id: usize) -> Self {
        Self::new(id, vec![])
    }
}

#[tokio::test]
async fn test_bounded_traversal() -> Result<(), Error> {
    let tree = build_tree();

    let tick = Tick::new();
    let log: StateLog<String> = StateLog::new();
    let reference: StateLog<String> = StateLog::new();

    let traverse = bounded_traversal(
        2, // level of parallelism
        tree,
        // unfold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |Tree { id, children }| {
                let log = log.clone();
                tick.sleep(1)
                    .map(move |now| {
                        log.unfold(id, now);
                        Ok::<_, Error>((id, children))
                    })
                    .boxed()
            }
        },
        // fold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id, children| {
                let log = log.clone();
                tick.sleep(1)
                    .map(move |now| {
                        let value = id.to_string() + &children.collect::<String>();
                        log.fold(id, now, value.clone());
                        Ok::<_, Error>(value)
                    })
                    .boxed()
            }
        },
    )
    .boxed();
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    // only two unfolds executet because of the parallelism constraint
    tick.tick().await;
    reference.unfold(5, 3);
    reference.unfold(4, 3);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(4, 4, "4".to_string());
    reference.fold(5, 4, "5".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(1, 5, "15".to_string());
    reference.unfold(3, 5);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(3, 6, "3".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(2, 7, "234".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(0, 8, "015234".to_string());
    assert_eq!(log, reference);

    assert_eq!(handle.await??, "015234");
    Ok(())
}

#[tokio::test]
async fn test_bounded_traversal_dag() -> Result<(), Error> {
    // dag
    //   0
    //  / \
    // 1   2
    //  \ / \
    //   3   4
    //  / \
    // 5   6
    //  \ /
    //   7
    //   |
    //   4 - will be resolved by the time it is reached
    let dag = hashmap! {
        0 => vec![1, 2],
        1 => vec![3],
        2 => vec![3, 4],
        3 => vec![5, 6],
        4 => vec![],
        5 => vec![7],
        6 => vec![7],
        7 => vec![4],
    };

    let tick = Tick::new();
    let log: StateLog<String> = StateLog::new();
    let reference: StateLog<String> = StateLog::new();

    let traverse = bounded_traversal_dag(
        2, // level of parallelism
        0,
        // unfold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id| {
                let log = log.clone();
                let children = dag.get(&id).cloned().unwrap_or_default();
                tick.sleep(1)
                    .map(move |now| {
                        log.unfold(id, now);
                        Ok::<_, Error>((id, children))
                    })
                    .boxed()
            }
        },
        // fold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id, children| {
                let log = log.clone();
                tick.sleep(1)
                    .map(move |now| {
                        let value = id.to_string() + &children.collect::<String>();
                        log.fold(id, now, value.clone());
                        Ok(value)
                    })
                    .boxed()
            }
        },
    )
    .boxed();
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(3, 3);
    reference.unfold(4, 3);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(4, 4, "4".to_string());
    reference.unfold(6, 4);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(5, 5);
    reference.unfold(7, 5);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(7, 6, "74".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(5, 7, "574".to_string());
    reference.fold(6, 7, "674".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(3, 8, "3574674".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(1, 9, "13574674".to_string());
    reference.fold(2, 9, "235746744".to_string());
    assert_eq!(log, reference);

    tick.tick().await;
    reference.fold(0, 10, "013574674235746744".to_string());
    assert_eq!(log, reference);

    assert_eq!(handle.await??, Some("013574674235746744".to_string()));
    Ok(())
}

#[tokio::test]
async fn test_bounded_traversal_dag_with_cycle() -> Result<(), Error> {
    // graph with cycle
    //   0
    //  / \
    // 1   2
    //  \ /
    //   3
    //   |
    //   2 <- forms cycle
    let graph = hashmap! {
        0 => vec![1, 2],
        1 => vec![3],
        2 => vec![3],
        3 => vec![2],
    };

    let tick = Tick::new();
    let log: StateLog<String> = StateLog::new();
    let reference: StateLog<String> = StateLog::new();

    let traverse = bounded_traversal_dag(
        2, // level of parallelism
        0,
        // unfold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id| {
                let log = log.clone();
                let children = graph.get(&id).cloned().unwrap_or_default();
                tick.sleep(1)
                    .map(move |now| {
                        log.unfold(id, now);
                        Ok::<_, Error>((id, children))
                    })
                    .boxed()
            }
        },
        // fold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id, children| {
                let log = log.clone();
                tick.sleep(1)
                    .map(move |now| {
                        let value = id.to_string() + &children.collect::<String>();
                        log.fold(id, now, value.clone());
                        Ok(value)
                    })
                    .boxed()
            }
        },
    )
    .boxed();
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(3, 3);
    assert_eq!(log, reference);

    assert_eq!(handle.await??, None); // cycle detected
    Ok(())
}

fn build_tree() -> Tree {
    // tree
    //      0
    //     / \
    //    1   2
    //   /   / \
    //  5   3   4
    Tree::new(
        0,
        vec![
            Tree::new(1, vec![Tree::leaf(5)]),
            Tree::new(2, vec![Tree::leaf(3), Tree::leaf(4)]),
        ],
    )
}

fn build_duplicates() -> Tree {
    Tree::new(
        0,
        vec![
            Tree::leaf(1),
            Tree::leaf(1),
            Tree::leaf(2),
            Tree::leaf(2),
            Tree::leaf(2),
        ],
    )
}

async fn check_stream_unfold_ticks<TestFn, Outs>(test_fn: TestFn) -> Result<(), Error>
where
    TestFn: FnOnce(Tree, Tick, StateLog<BTreeSet<usize>>) -> Outs,
    Outs: Stream<Item = Result<usize, Error>> + Send + 'static,
{
    let tree = build_tree();

    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();

    let traverse = test_fn(tree, tick.clone(), log.clone())
        .try_collect::<BTreeSet<usize>>()
        .boxed();
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(5, 3);
    reference.unfold(4, 3);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(3, 4);
    assert_eq!(log, reference);

    assert_eq!(handle.await??, (0..6).collect::<BTreeSet<_>>());
    Ok(())
}

async fn check_duplicate_stream_unfold_ticks<TestFn, Outs>(test_fn: TestFn) -> Result<(), Error>
where
    TestFn: FnOnce(Tree, Tick, StateLog<BTreeSet<usize>>) -> Outs,
    Outs: Stream<Item = Result<usize, Error>> + Send + 'static,
{
    let tree = build_duplicates();

    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();

    let traverse = test_fn(tree, tick.clone(), log.clone())
        .try_collect::<BTreeSet<usize>>()
        .boxed();
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(2, 2);
    reference.unfold(1, 2);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(2, 3);
    reference.unfold(1, 3);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(2, 4);
    assert_eq!(log, reference);

    assert_eq!(handle.await??, (0..3).collect::<BTreeSet<_>>());
    Ok(())
}

// Test the parents observed for each item.
// This is similar to check_stream_unfold_ticks, but it assumes test_fn is putting the parent id
// in the log instead of the tick number
async fn check_stream_unfold_parents<TestFn, Outs>(test_fn: TestFn) -> Result<(), Error>
where
    TestFn: FnOnce(Tree, Tick, StateLog<BTreeSet<usize>>) -> Outs,
    Outs: Stream<Item = Result<usize, Error>> + Send + 'static,
{
    let tree = build_tree();

    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();

    let traverse = test_fn(tree, tick.clone(), log.clone())
        .try_collect::<BTreeSet<usize>>()
        .boxed();
    let handle = tokio::spawn(traverse);

    yield_now().await;
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(0, 0);
    assert_eq!(log, reference);

    // Important part of this is that 2 unfold per tick if possible
    // and that tree structure matches the diagram above
    tick.tick().await;
    reference.unfold(1, 0);
    reference.unfold(2, 0);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(4, 2);
    reference.unfold(5, 1);
    assert_eq!(log, reference);

    tick.tick().await;
    reference.unfold(3, 2);
    assert_eq!(log, reference);

    assert_eq!(handle.await??, (0..6).collect::<BTreeSet<_>>());
    Ok(())
}

#[tokio::test]
async fn test_bounded_traversal_stream_ticks() -> Result<(), Error> {
    check_stream_unfold_ticks(|tree, tick, log| {
        bounded_traversal_stream(2, Some(tree), {
            cloned!(tick, log);
            move |Tree { id, children }| {
                cloned!(log);
                tick.sleep(1)
                    .map(move |now| {
                        log.unfold(id, now);
                        Ok::<_, Error>((id, children))
                    })
                    .boxed()
            }
        })
    })
    .await
}

#[tokio::test]
async fn test_bounded_traversal_stream_parents() -> Result<(), Error> {
    check_stream_unfold_parents(|tree, tick, log| {
        bounded_traversal_stream(2, Some((tree, 0)), {
            cloned!(tick, log);
            move |(Tree { id, children }, parent)| {
                cloned!(log);
                tick.sleep(1)
                    .map(move |_now| {
                        log.unfold(id, parent);
                        Ok::<_, Error>((id, children.into_iter().map(move |i| (i, id))))
                    })
                    .boxed()
            }
        })
    })
    .await
}

#[tokio::test]
async fn test_limited_by_key_shardable_ticks() -> Result<(), Error> {
    check_stream_unfold_ticks(|tree, tick, log| {
        limited_by_key_shardable(
            2,
            Some(tree),
            {
                cloned!(tick, log);
                move |Tree { id, children }| {
                    cloned!(log);
                    tick.sleep(1)
                        .map(move |now| {
                            log.unfold(id, now);
                            (id, None::<()>, Ok::<_, Error>(Some((id, children))))
                        })
                        .boxed()
                }
            },
            |item| (&item.id, None),
        )
    })
    .await
}

#[tokio::test]
async fn test_limited_by_key_shardable_duplicate_ticks() -> Result<(), Error> {
    check_duplicate_stream_unfold_ticks(|tree, tick, log| {
        limited_by_key_shardable(
            2,
            Some(tree),
            {
                cloned!(tick, log);
                move |Tree { id, children }| {
                    cloned!(log);
                    tick.sleep(1)
                        .map(move |now| {
                            log.unfold(id, now);
                            (id, None::<()>, Ok::<_, Error>(Some((id, children))))
                        })
                        .boxed()
                }
            },
            |item| (&item.id, None),
        )
    })
    .await
}

#[tokio::test]
async fn test_limited_by_key_shardable_duplicate_ticks_sharded() -> Result<(), Error> {
    check_duplicate_stream_unfold_ticks(|tree, tick, log| {
        limited_by_key_shardable(
            10, // This has a wide executor
            Some(tree),
            {
                cloned!(tick, log);
                move |Tree { id, children }| {
                    cloned!(log);
                    tick.sleep(1)
                        .map(move |now| {
                            log.unfold(id, now);
                            (id, Some(()), Ok::<_, Error>(Some((id, children))))
                        })
                        .boxed()
                }
            },
            // But max 2 active keys per shard means it matches same log as test_limited_by_key_shardable_duplicate_ticks
            |item| (&item.id, Some(((), 2))),
        )
    })
    .await
}

#[tokio::test]
async fn test_limited_by_key_shardable_parents() -> Result<(), Error> {
    check_stream_unfold_parents(|tree, tick, log| {
        limited_by_key_shardable(
            2,
            Some((tree, 0)),
            {
                cloned!(tick, log);
                move |(Tree { id, children }, parent)| {
                    cloned!(log);
                    tick.sleep(1)
                        .map(move |_now| {
                            log.unfold(id, parent);
                            (
                                id,
                                None::<()>,
                                Ok::<_, Error>(Some((
                                    id,
                                    children.into_iter().map(move |i| (i, id)),
                                ))),
                            )
                        })
                        .boxed()
                }
            },
            |(item, _parent)| (&item.id, None),
        )
    })
    .await
}
