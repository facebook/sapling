// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::{
    common::{Job, JobResult},
    Iter,
};
use futures::{stream::FuturesUnordered, try_ready, Async, Future, IntoFuture, Poll, Stream};
use std::collections::{HashMap, VecDeque};

/// `bounded_traversal` traverses implicit asynchronous tree specified by `init`
/// and `unfold` arguments, and it also does backward pass with `fold` operation.
/// All `unfold` and `fold` operations are executed in parallel if they do not
/// depend on each other (not related by ancestor-descendant relation in implicit tree)
/// with amount of concurrency constrained by `scheduled_max`.
///
/// ## `init: In`
/// Is the root of the implicit tree to be traversed
///
/// ## `unfold: FnMut(In) -> impl IntoFuture<Item = (OutCtx, impl IntoIterator<Item = In>)>`
/// Asynchronous function which given input value produces list of its children. And context
/// associated with current node. If this list is empty, it is a leaf of the tree, and `fold`
/// will be run on this node.
///
/// ## `fold: FnMut(OutCtx, impl Iterator<Out>) -> impl IntoFuture<Item=Out>`
/// Aynchronous function which given node context and output of `fold` for its chidlren
/// should produce new output value.
///
/// ## return value `impl Future<Item = Out>`
/// Result of running fold operation on the root of the tree.
///
pub fn bounded_traversal<In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut>(
    scheduled_max: usize,
    init: In,
    unfold: Unfold,
    fold: Fold,
) -> impl Future<Item = Out, Error = UFut::Error>
where
    Unfold: FnMut(In) -> UFut,
    UFut: IntoFuture<Item = (OutCtx, Ins)>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
    FFut: IntoFuture<Item = Out, Error = UFut::Error>,
{
    BoundedTraversal::new(scheduled_max, init, unfold, fold)
}

// execution tree node
struct Node<Out, OutCtx> {
    parent: NodeLocation,       // location of this node relative to it's parent
    context: OutCtx,            // context associated with node
    children: Vec<Option<Out>>, // results of children folds
    children_left: usize,       // number of unresolved children
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct NodeIndex(usize);
type NodeLocation = super::common::NodeLocation<NodeIndex>;

#[must_use = "futures do nothing unless polled"]
struct BoundedTraversal<Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    UFut: IntoFuture,
    FFut: IntoFuture,
{
    unfold: Unfold,
    fold: Fold,
    scheduled_max: usize,
    scheduled: FuturesUnordered<Job<NodeLocation, UFut::Future, FFut::Future>>, // jobs being executed
    unscheduled: VecDeque<Job<NodeLocation, UFut::Future, FFut::Future>>, // as of yet unscheduled jobs
    execution_tree: HashMap<NodeIndex, Node<Out, OutCtx>>, // tree tracking execution process
    execution_tree_index: NodeIndex,                       // last allocated node index
}

impl<In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut>
    BoundedTraversal<Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    Unfold: FnMut(In) -> UFut,
    UFut: IntoFuture<Item = (OutCtx, Ins)>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
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
        self.unscheduled.push_front(Job::Unfold {
            value: parent,
            future: (self.unfold)(value).into_future(),
        });
    }

    fn enqueue_fold(&mut self, parent: NodeLocation, context: OutCtx, children: Iter<Out>) {
        self.unscheduled.push_front(Job::Fold {
            value: parent,
            future: (self.fold)(context, children).into_future(),
        });
    }

    fn process_unfold(&mut self, parent: NodeLocation, (context, children): UFut::Item) {
        // allocate index
        self.execution_tree_index = NodeIndex(self.execution_tree_index.0 + 1);
        let node_index = self.execution_tree_index;

        // schedule unfold for node's children
        let count = children.into_iter().fold(0, |child_index, child| {
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
            // allocate node
            let mut children = Vec::new();
            children.resize_with(count, || None);
            self.execution_tree.insert(
                node_index,
                Node {
                    parent,
                    context,
                    children,
                    children_left: count,
                },
            );
        } else {
            // leaf node schedules fold for itself immediately
            self.enqueue_fold(parent, context, Vec::new().into_iter().flatten());
        }
    }

    fn process_fold(&mut self, parent: NodeLocation, result: Out) {
        if {
            // update parent
            let node = self
                .execution_tree
                .get_mut(&parent.node_index)
                .expect("fold referenced invalid node");
            debug_assert!(node.children[parent.child_index].is_none());
            node.children[parent.child_index] = Some(result);
            node.children_left -= 1;
            node.children_left == 0
        } {
            // all parents children have been completed, so we need
            // to schedule fold operation for it
            let Node {
                parent,
                context,
                children,
                ..
            } = self
                .execution_tree
                .remove(&parent.node_index)
                .expect("fold referenced invalid node");
            self.enqueue_fold(parent, context, children.into_iter().flatten());
        }
    }
}

impl<In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut> Future
    for BoundedTraversal<Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    Unfold: FnMut(In) -> UFut,
    UFut: IntoFuture<Item = (OutCtx, Ins)>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
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
                    JobResult::Unfold { value, result } => self.process_unfold(value, result),
                    JobResult::Fold { value, result } => {
                        // `0` is special index which means whole tree have been executed
                        if value.node_index == NodeIndex(0) {
                            // all jobs have to be completed and execution_tree empty
                            assert!(self.execution_tree.is_empty());
                            assert!(self.unscheduled.is_empty());
                            assert!(self.scheduled.is_empty());
                            return Ok(Async::Ready(result));
                        }
                        self.process_fold(value, result);
                    }
                }
            }
        }
    }
}
