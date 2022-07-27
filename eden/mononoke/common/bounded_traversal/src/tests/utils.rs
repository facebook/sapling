/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Error;
use futures::channel::oneshot::channel;
use futures::channel::oneshot::Sender;
use futures::future;
use futures::future::Future;
use futures::future::FutureExt;
use lock_ext::LockExt;
use pretty_assertions::assert_eq;
use tokio::task::yield_now;

// Manully controlled timer
pub struct TickInner {
    current_time: usize,
    events: BinaryHeap<TickEvent>,
}

#[derive(Clone)]
pub struct Tick {
    inner: Arc<Mutex<TickInner>>,
}

pub struct TickEvent {
    time: usize,
    sender: Sender<usize>,
}

impl PartialOrd for TickEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TickInner {
                current_time: 0,
                events: BinaryHeap::new(),
            })),
        }
    }

    pub async fn tick(&self) {
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
        yield_now().await
    }

    pub fn sleep(&self, delay: usize) -> impl Future<Output = usize> {
        let this = self.clone();
        async move {
            let (send, recv) = channel();
            this.inner.with(move |inner| {
                inner.events.push(TickEvent {
                    time: inner.current_time + delay,
                    sender: send,
                });
            });
            recv.await.expect("peer closed")
        }
    }
}

// log for recording and comparing events
#[derive(Debug, Eq, PartialEq, Hash, Clone, Ord, PartialOrd)]
enum State<V> {
    Unfold { id: usize, time: usize },
    Fold { id: usize, time: usize, value: V },
}

#[derive(Clone, Debug)]
pub struct StateLog<V: Ord> {
    states: Arc<Mutex<BTreeSet<State<V>>>>,
}

impl<V: Ord> StateLog<V> {
    pub fn new() -> Self {
        Self {
            states: Default::default(),
        }
    }

    pub fn fold(&self, id: usize, time: usize, value: V) {
        self.states
            .with(move |states| states.insert(State::Fold { id, time, value }));
    }

    pub fn unfold(&self, id: usize, time: usize) {
        self.states
            .with(move |states| states.insert(State::Unfold { id, time }));
    }
}

impl<V: Ord + Clone> PartialEq for StateLog<V> {
    fn eq(&self, other: &Self) -> bool {
        self.states.with(|s| s.clone()) == other.states.with(|s| s.clone())
    }
}

#[tokio::test]
async fn test_tick() -> Result<(), Error> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut reference = Vec::new();
    let tick = Tick::new();

    let handle = tokio::spawn({
        let log = log.clone();
        let tick = tick.clone();
        async move {
            let f0 = tick.sleep(3).map(|t| log.with(|l| l.push((3usize, t))));
            let f1 = tick.sleep(1).map(|t| log.with(|l| l.push((1usize, t))));
            let f2 = tick.sleep(2).map(|t| log.with(|l| l.push((2usize, t))));
            future::join3(f0, f1, f2).await;
        }
    });
    yield_now().await;

    tick.tick().await;
    reference.push((1usize, 1usize));
    assert_eq!(log.with(|l| l.clone()), reference);

    tick.tick().await;
    reference.push((2, 2));
    assert_eq!(log.with(|l| l.clone()), reference);

    tick.tick().await;
    reference.push((3, 3));
    assert_eq!(log.with(|l| l.clone()), reference);

    handle.await?;
    Ok(())
}
