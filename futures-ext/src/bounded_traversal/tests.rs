/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::{bounded_traversal, bounded_traversal_dag, bounded_traversal_stream};
use failure::Error;
use futures::{
    future,
    sync::oneshot::{channel, Sender},
    Future, Stream,
};
use lock_ext::LockExt;
use maplit::hashmap;
use pretty_assertions::assert_eq;
use std::{
    cmp::{Ord, Ordering},
    collections::{BTreeSet, BinaryHeap},
    iter::FromIterator,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::runtime::Runtime;

// Tree for test purposes
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

// Manully controlled timer
struct TickInner {
    current_time: usize,
    events: BinaryHeap<TickEvent>,
}

#[derive(Clone)]
struct Tick {
    inner: Arc<Mutex<TickInner>>,
}

struct TickEvent {
    time: usize,
    sender: Sender<usize>,
}

impl PartialOrd for TickEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for TickEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.cmp(&other.time).reverse()
    }
}

impl PartialEq for TickEvent {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl Eq for TickEvent {}

impl Tick {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TickInner {
                current_time: 0,
                events: BinaryHeap::new(),
            })),
        }
    }

    fn tick(&self) {
        let (current_time, done) = self.inner.with(|inner| {
            inner.current_time += 1;
            let mut done = Vec::new();
            while let Some(event) = inner.events.pop() {
                if event.time <= inner.current_time {
                    done.push(event.sender)
                } else {
                    inner.events.push(event);
                    break;
                }
            }
            (inner.current_time, done)
        });
        for sender in done {
            sender.send(current_time).unwrap();
        }
    }

    fn sleep(&self, delay: usize) -> impl Future<Item = usize, Error = ()> {
        let this = self.clone();
        future::lazy(move || {
            let (send, recv) = channel();
            this.inner.with(move |inner| {
                inner.events.push(TickEvent {
                    time: inner.current_time + delay,
                    sender: send,
                });
            });
            recv.map_err(|_| ())
        })
    }
}

// log for recording and comparing events
#[derive(Debug, Eq, PartialEq, Hash, Clone, Ord, PartialOrd)]
enum State<V> {
    Unfold { id: usize, time: usize },
    Fold { id: usize, time: usize, value: V },
    Done { value: Option<V> },
}

#[derive(Clone, Debug)]
struct StateLog<V: Ord> {
    states: Arc<Mutex<BTreeSet<State<V>>>>,
}

impl<V: Ord> StateLog<V> {
    fn new() -> Self {
        Self {
            states: Default::default(),
        }
    }

    fn fold(&self, id: usize, time: usize, value: V) {
        self.states
            .with(move |states| states.insert(State::Fold { id, time, value }));
    }

    fn unfold(&self, id: usize, time: usize) {
        self.states
            .with(move |states| states.insert(State::Unfold { id, time }));
    }

    fn done(&self, value: Option<V>) {
        self.states
            .with(move |states| states.insert(State::Done { value }));
    }
}

impl<V: Ord + Clone> PartialEq for StateLog<V> {
    fn eq(&self, other: &Self) -> bool {
        self.states.with(|s| s.clone()) == other.states.with(|s| s.clone())
    }
}

#[test]
fn test_tick() -> Result<(), Error> {
    use futures::stream::{FuturesUnordered, Stream};

    let log = Arc::new(Mutex::new(Vec::new()));
    let mut reference = Vec::new();
    let tick = Tick::new();
    let mut runtime = Runtime::new()?;

    let mut futs: FuturesUnordered<Box<dyn Future<Item = (), Error = ()> + Sync + Send>> =
        FuturesUnordered::new();
    futs.push(Box::new(tick.sleep(3).map({
        let log = log.clone();
        move |t| log.with(|l| l.push((3, t)))
    })));
    futs.push(Box::new(tick.sleep(1).map({
        let log = log.clone();
        move |t| log.with(|l| l.push((1, t)))
    })));
    futs.push(Box::new(tick.sleep(2).map({
        let log = log.clone();
        move |t| log.with(|l| l.push((2, t)))
    })));
    runtime.spawn(futs.for_each(|_| Ok(())));
    thread::sleep(Duration::from_millis(50));

    let tick = move || {
        tick.tick();
        thread::sleep(Duration::from_millis(50));
    };

    tick();
    reference.push((1, 1));
    assert_eq!(log.with(|l| l.clone()), reference);

    tick();
    reference.push((2, 2));
    assert_eq!(log.with(|l| l.clone()), reference);

    tick();
    reference.push((3, 3));
    assert_eq!(log.with(|l| l.clone()), reference);

    Ok(())
}

#[test]
fn test_bounded_traversal() -> Result<(), Error> {
    // tree
    //      0
    //     / \
    //    1   2
    //   /   / \
    //  5   3   4
    let tree = Tree::new(
        0,
        vec![
            Tree::new(1, vec![Tree::leaf(5)]),
            Tree::new(2, vec![Tree::leaf(3), Tree::leaf(4)]),
        ],
    );

    let tick = Tick::new();
    let log: StateLog<String> = StateLog::new();
    let reference: StateLog<String> = StateLog::new();
    let mut rt = Runtime::new()?;

    let traverse = bounded_traversal(
        2, // level of parallelism
        tree,
        // unfold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |Tree { id, children }| {
                let log = log.clone();
                tick.sleep(1).map(move |now| {
                    log.unfold(id, now);
                    (id, children)
                })
            }
        },
        // fold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id, children| {
                let log = log.clone();
                tick.sleep(1).map(move |now| {
                    let value = id.to_string() + &children.into_iter().collect::<String>();
                    log.fold(id, now, value.clone());
                    value
                })
            }
        },
    );
    rt.spawn(traverse.map({
        let log = log.clone();
        move |value| log.done(Some(value))
    }));

    let tick = move || {
        tick.tick();
        thread::sleep(Duration::from_millis(50));
    };

    thread::sleep(Duration::from_millis(50));
    assert_eq!(log, reference);

    tick();
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick();
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    // only two unfolds executet because of the parallelism constraint
    tick();
    reference.unfold(5, 3);
    reference.unfold(4, 3);
    assert_eq!(log, reference);

    tick();
    reference.fold(4, 4, "4".to_string());
    reference.fold(5, 4, "5".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(1, 5, "15".to_string());
    reference.unfold(3, 5);
    assert_eq!(log, reference);

    tick();
    reference.fold(3, 6, "3".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(2, 7, "234".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(0, 8, "015234".to_string());
    reference.done(Some("015234".to_string()));
    assert_eq!(log, reference);

    Ok(())
}

#[test]
fn test_bounded_traversal_dag() -> Result<(), Error> {
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
    let mut rt = Runtime::new()?;

    let traverse = bounded_traversal_dag(
        2, // level of parallelism
        0,
        // unfold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id| {
                let log = log.clone();
                let children = dag.get(&id).cloned().unwrap_or(Vec::new());
                tick.sleep(1).map(move |now| {
                    log.unfold(id, now);
                    (id, children)
                })
            }
        },
        // fold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id, children| {
                let log = log.clone();
                tick.sleep(1).map(move |now| {
                    let value = id.to_string() + &children.into_iter().collect::<String>();
                    log.fold(id, now, value.clone());
                    value
                })
            }
        },
    );
    rt.spawn(traverse.map({
        let log = log.clone();
        move |value| log.done(value)
    }));

    let tick = move || {
        tick.tick();
        thread::sleep(Duration::from_millis(50));
    };

    thread::sleep(Duration::from_millis(50));
    assert_eq!(log, reference);

    tick();
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick();
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    tick();
    reference.unfold(3, 3);
    reference.unfold(4, 3);
    assert_eq!(log, reference);

    tick();
    reference.fold(4, 4, "4".to_string());
    reference.unfold(6, 4);
    assert_eq!(log, reference);

    tick();
    reference.unfold(5, 5);
    reference.unfold(7, 5);
    assert_eq!(log, reference);

    tick();
    reference.fold(7, 6, "74".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(5, 7, "574".to_string());
    reference.fold(6, 7, "674".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(3, 8, "3574674".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(1, 9, "13574674".to_string());
    reference.fold(2, 9, "235746744".to_string());
    assert_eq!(log, reference);

    tick();
    reference.fold(0, 10, "013574674235746744".to_string());
    reference.done(Some("013574674235746744".to_string()));
    assert_eq!(log, reference);
    Ok(())
}

#[test]
fn test_bounded_traversal_dag_with_cycle() -> Result<(), Error> {
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
    let mut rt = Runtime::new()?;

    let traverse = bounded_traversal_dag(
        2, // level of parallelism
        0,
        // unfold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id| {
                let log = log.clone();
                let children = graph.get(&id).cloned().unwrap_or(Vec::new());
                tick.sleep(1).map(move |now| {
                    log.unfold(id, now);
                    (id, children)
                })
            }
        },
        // fold
        {
            let tick = tick.clone();
            let log = log.clone();
            move |id, children| {
                let log = log.clone();
                tick.sleep(1).map(move |now| {
                    let value = id.to_string() + &children.into_iter().collect::<String>();
                    log.fold(id, now, value.clone());
                    value
                })
            }
        },
    );
    rt.spawn(traverse.map({
        let log = log.clone();
        move |value| log.done(value)
    }));

    let tick = move || {
        tick.tick();
        thread::sleep(Duration::from_millis(50));
    };

    thread::sleep(Duration::from_millis(50));
    assert_eq!(log, reference);

    tick();
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick();
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    tick();
    reference.unfold(3, 3);
    reference.done(None); // cycle detected
    assert_eq!(log, reference);

    Ok(())
}

#[test]
fn test_bounded_traversal_stream() -> Result<(), Error> {
    // tree
    //      0
    //     / \
    //    1   2
    //   /   / \
    //  5   3   4
    let tree = Tree::new(
        0,
        vec![
            Tree::new(1, vec![Tree::leaf(5)]),
            Tree::new(2, vec![Tree::leaf(3), Tree::leaf(4)]),
        ],
    );

    let tick = Tick::new();
    let log: StateLog<BTreeSet<usize>> = StateLog::new();
    let reference: StateLog<BTreeSet<usize>> = StateLog::new();
    let mut rt = Runtime::new()?;

    let traverse = bounded_traversal_stream(2, Some(tree), {
        let tick = tick.clone();
        let log = log.clone();
        move |Tree { id, children }| {
            let log = log.clone();
            tick.sleep(1).map(move |now| {
                log.unfold(id, now);
                (id, children)
            })
        }
    });
    rt.spawn(traverse.collect().map({
        let log = log.clone();
        move |items| log.done(Some(BTreeSet::from_iter(items)))
    }));

    let tick = move || {
        tick.tick();
        thread::sleep(Duration::from_millis(50));
    };

    thread::sleep(Duration::from_millis(50));
    assert_eq!(log, reference);

    tick();
    reference.unfold(0, 1);
    assert_eq!(log, reference);

    tick();
    reference.unfold(1, 2);
    reference.unfold(2, 2);
    assert_eq!(log, reference);

    tick();
    reference.unfold(5, 3);
    reference.unfold(4, 3);
    assert_eq!(log, reference);

    tick();
    reference.unfold(3, 4);
    reference.done(Some(BTreeSet::from_iter(0..6)));
    assert_eq!(log, reference);

    Ok(())
}
