// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{stream::FuturesUnordered, try_ready, Async, Future, IntoFuture, Poll, Stream};
use std::collections::{HashMap, VecDeque};

pub type Iter<Out> = std::iter::Flatten<std::vec::IntoIter<Option<Out>>>;

/// `bounded_traversal` traverses implicit asynchronous tree specified by `init`
/// and `unfold` arguments, and it also does backward pass with `fold` operation.
/// All `unfold` and `fold` operations are executed in parallel if they do not
/// depend on each other (not related by ancestor-descendant relation in implicit tree)
/// with amount of concurrency constrained by `scheduled_max`.
///
/// ## `init: In`
/// Is the root of the implicit tree to be traversed
///
/// ## `unfold: FnMut(&In) -> impl IntoFuture<Item = impl IntoIterator<Item = In>>`
/// Asynchronous function which given input value produces list of its children.
/// If this list is empty it is a leaf of the tree and `fold` can be run on this node.
///
/// ## `fold: FnMut(In, impl Iterator<Out>) -> impl IntoFuture<Item=Out>`
/// Aynchronous function which given input node and output of `fold` for its chidlren
/// should produce new output value.
///
/// ## return value `impl Future<Item = Out>`
/// Result of running fold operation on the root of the tree.
///
pub fn bounded_traversal<In, Out, Unfold, UFut, Fold, FFut>(
    scheduled_max: usize,
    init: In,
    unfold: Unfold,
    fold: Fold,
) -> impl Future<Item = Out, Error = UFut::Error>
where
    Unfold: FnMut(&In) -> UFut,
    UFut: IntoFuture,
    UFut::Item: IntoIterator<Item = In>,
    Fold: FnMut(In, Iter<Out>) -> FFut,
    FFut: IntoFuture<Item = Out, Error = UFut::Error>,
{
    BoundedTraversal::new(scheduled_max, init, unfold, fold)
}

// execution tree node
struct Node<In, Out> {
    parent: NodeLocation,       // location of this node relative to it's parent
    value: In,                  // value associated with node
    children: Vec<Option<Out>>, // results of children folds
    children_left: usize,       // number of unresolved children
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct NodeIndex(usize);

#[derive(Clone, Copy)]
struct NodeLocation {
    node_index: NodeIndex, // node index inside execution tree
    child_index: usize,    // index inside parents children list
}

struct BoundedTraversal<In, Out, Unfold, UFut, Fold, FFut>
where
    UFut: IntoFuture,
    FFut: IntoFuture,
{
    unfold: Unfold,
    fold: Fold,
    scheduled_max: usize,
    scheduled: FuturesUnordered<Job<UFut::Future, FFut::Future>>, // jobs being executed
    unscheduled: VecDeque<Job<UFut::Future, FFut::Future>>,       // as of yet unscheduled jobs
    execution_tree: HashMap<NodeIndex, Node<In, Out>>,            // tree tracking execution process
    execution_tree_index: NodeIndex,                              // last allocated node index
}

impl<In, Out, Unfold, UFut, Fold, FFut> BoundedTraversal<In, Out, Unfold, UFut, Fold, FFut>
where
    Unfold: FnMut(&In) -> UFut,
    UFut: IntoFuture,
    UFut::Item: IntoIterator<Item = In>,
    Fold: FnMut(In, Iter<Out>) -> FFut,
    FFut: IntoFuture<Item = Out, Error = UFut::Error>,
{
    fn new(scheduled_max: usize, init: In, unfold: Unfold, fold: Fold) -> Self {
        let mut this = Self {
            unfold,
            fold,
            scheduled_max,
            scheduled: FuturesUnordered::new(),
            unscheduled: VecDeque::new(),
            execution_tree: HashMap::new(),
            execution_tree_index: NodeIndex(0),
        };
        this.enqueue_unfold(
            NodeLocation {
                node_index: NodeIndex(0),
                child_index: 0,
            },
            init,
        );
        this
    }

    fn enqueue_unfold(&mut self, parent: NodeLocation, value: In) {
        // allocate index
        self.execution_tree_index = NodeIndex(self.execution_tree_index.0 + 1);
        let node_index = self.execution_tree_index;
        // create future
        let future = (self.unfold)(&value).into_future();
        // allocate execution node
        self.execution_tree.insert(
            node_index,
            Node {
                parent,
                value,
                children: Vec::new(),
                children_left: 0,
            },
        );
        // enqueue job
        self.unscheduled
            .push_front(Job::Unfold { node_index, future });
    }

    fn enqueue_fold(&mut self, parent: NodeLocation, value: In, children: Iter<Out>) {
        self.unscheduled.push_front(Job::Fold {
            parent,
            future: (self.fold)(value, children).into_future(),
        });
    }

    fn process_unfold(&mut self, node_index: NodeIndex, result: UFut::Item) {
        // schedule unfold for node's children
        let count = result.into_iter().fold(0, |child_index, child| {
            self.enqueue_unfold(
                NodeLocation {
                    node_index,
                    child_index,
                },
                child,
            );
            child_index + 1
        });
        if count != 0 {
            // preallocate storage for children's folds
            let node = self
                .execution_tree
                .get_mut(&node_index)
                .expect("unfold referenced invalid node");
            node.children_left = count;
            node.children.resize_with(count, || None);
        } else {
            // leaf node schedules fold for itself
            let Node { parent, value, .. } = self
                .execution_tree
                .remove(&node_index)
                .expect("unfold referenced invalid node");
            self.enqueue_fold(parent, value, Vec::new().into_iter().flatten());
        }
    }

    fn process_fold(&mut self, parent: NodeLocation, result: Out) {
        // update parent
        let node = self
            .execution_tree
            .get_mut(&parent.node_index)
            .expect("fold referenced invalid node");
        debug_assert!(node.children[parent.child_index].is_none());
        node.children[parent.child_index] = Some(result);
        node.children_left -= 1;

        if node.children_left == 0 {
            // all parents children have been completed, so we need
            // to schedule fold operation for it
            let Node {
                parent,
                value,
                children,
                ..
            } = self
                .execution_tree
                .remove(&parent.node_index)
                .expect("fold referenced invalid node");
            self.enqueue_fold(parent, value, children.into_iter().flatten());
        }
    }
}

impl<In, Out, Unfold, UFut, Fold, FFut> Future
    for BoundedTraversal<In, Out, Unfold, UFut, Fold, FFut>
where
    Unfold: FnMut(&In) -> UFut,
    UFut: IntoFuture,
    UFut::Item: IntoIterator<Item = In>,
    Fold: FnMut(In, Iter<Out>) -> FFut,
    FFut: IntoFuture<Item = Out, Error = UFut::Error>,
{
    type Item = Out;
    type Error = UFut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            // schedule as many jobs as possible
            for job in self.unscheduled.drain(
                ..std::cmp::min(
                    self.unscheduled.len(),
                    self.scheduled_max - self.scheduled.len(),
                ),
            ) {
                self.scheduled.push(job);
            }

            // execute scheduled until it is blocked or done
            if let Some(job_result) = try_ready!(self.scheduled.poll()) {
                match job_result {
                    JobResult::Unfold { node_index, result } => {
                        self.process_unfold(node_index, result)
                    }
                    JobResult::Fold { parent, result } => {
                        // `0` is special index which means whole tree have been executed
                        if parent.node_index == NodeIndex(0) {
                            // all jobs have to be completed and execution_tree empty
                            assert!(self.execution_tree.is_empty());
                            assert!(self.unscheduled.is_empty());
                            assert!(self.scheduled.is_empty());
                            return Ok(Async::Ready(result));
                        }
                        self.process_fold(parent, result);
                    }
                }
            }
        }
    }
}

// This is essentially just a `.map`  over futures `{FFut|UFut}`, this only exisists
// so it would be possible to name `FuturesUnoredered` type parameter.
enum Job<UFut, FFut> {
    Unfold { node_index: NodeIndex, future: UFut },
    Fold { parent: NodeLocation, future: FFut },
}

enum JobResult<In, Out> {
    Unfold { node_index: NodeIndex, result: In },
    Fold { parent: NodeLocation, result: Out },
}

impl<UFut, FFut> Future for Job<UFut, FFut>
where
    UFut: Future,
    FFut: Future<Error = UFut::Error>,
{
    type Item = JobResult<UFut::Item, FFut::Item>;
    type Error = FFut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let result = match self {
            Job::Fold { future, parent } => JobResult::Fold {
                parent: *parent,
                result: try_ready!(future.poll()),
            },
            Job::Unfold { future, node_index } => JobResult::Unfold {
                node_index: *node_index,
                result: try_ready!(future.poll()),
            },
        };
        Ok(Async::Ready(result))
    }
}

#[cfg(test)]
mod tests {
    use super::bounded_traversal;
    use futures::{
        future,
        sync::oneshot::{channel, Sender},
        Future,
    };
    use std::{
        cmp::{Ord, Ordering},
        collections::{BinaryHeap, HashSet},
        hash::Hash,
        ops::Deref,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };
    use tokio::runtime::Runtime;

    trait MutexExt {
        type Value;

        fn with<F, O>(&self, f: F) -> O
        where
            F: FnOnce(&mut Self::Value) -> O;
    }

    impl<T> MutexExt for Mutex<T> {
        type Value = T;

        fn with<F, O>(&self, f: F) -> O
        where
            F: FnOnce(&mut Self::Value) -> O,
        {
            let mut value = self.lock().expect("lock poisoned");
            f(&mut *value)
        }
    }

    // Tree for test purposes
    struct TreeInner {
        id: usize,
        children: Vec<Tree>,
    }

    #[derive(Clone)]
    struct Tree {
        inner: Arc<TreeInner>,
    }

    impl Tree {
        fn new(id: usize, children: Vec<Tree>) -> Self {
            Self {
                inner: Arc::new(TreeInner { id, children }),
            }
        }

        fn leaf(id: usize) -> Self {
            Self::new(id, vec![])
        }
    }

    impl Deref for Tree {
        type Target = TreeInner;

        fn deref(&self) -> &Self::Target {
            &*self.inner
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
    #[derive(Debug, Eq, PartialEq, Hash, Clone)]
    enum State<V> {
        Unfold { id: usize, time: usize },
        Fold { id: usize, time: usize, value: V },
        Done { value: V },
    }

    #[derive(Clone, Debug)]
    struct StateLog<V: Eq + Hash> {
        states: Arc<Mutex<HashSet<State<V>>>>,
    }

    impl<V: Eq + Hash> StateLog<V> {
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

        fn done(&self, value: V) {
            self.states
                .with(move |states| states.insert(State::Done { value }));
        }
    }

    impl<V: Eq + Hash + Clone> PartialEq for StateLog<V> {
        fn eq(&self, other: &Self) -> bool {
            self.states.with(|s| s.clone()) == other.states.with(|s| s.clone())
        }
    }

    impl<V: Eq + Hash + Clone> Eq for StateLog<V> {}

    #[test]
    fn test() -> Result<(), ()> {
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
        let mut rt = Runtime::new().expect("failed to create runtime");

        let traverse = bounded_traversal(
            2, // level of parallelism
            tree,
            // unfold
            {
                let tick = tick.clone();
                let log = log.clone();
                move |node| {
                    let node = node.clone();
                    let log = log.clone();
                    tick.sleep(1).map(move |now| {
                        log.unfold(node.id, now);
                        node.children.clone()
                    })
                }
            },
            // fold
            {
                let tick = tick.clone();
                let log = log.clone();
                move |node, children| {
                    let log = log.clone();
                    tick.sleep(1).map(move |now| {
                        let value = node.id.to_string() + &children.into_iter().collect::<String>();
                        log.fold(node.id, now, value.clone());
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
        reference.done("015234".to_string());
        assert_eq!(log, reference);

        Ok(())
    }
}
