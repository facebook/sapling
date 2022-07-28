/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use either::Either;
use futures::future::join;
use futures::future::ready;
use futures::future::BoxFuture;
use futures::future::Join;
use futures::future::Ready;
use futures::ready;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;

use super::common::Either2;
use super::Iter;

/// `bounded_traversal` traverses implicit asynchronous tree specified by `init`
/// and `unfold` arguments, and it also does backward pass with `fold` operation.
/// All `unfold` and `fold` operations are executed in parallel if they do not
/// depend on each other (not related by ancestor-descendant relation in implicit tree)
/// with amount of concurrency constrained by `scheduled_max`.
///
/// ## `init: In`
/// Is the root of the implicit tree to be traversed
///
/// ## `unfold: FnMut(In) -> impl Future<Output = Result<(OutCtx, impl IntoIterator<Item = In>), Err>>`
/// Asynchronous function which given input value produces list of its children. And context
/// associated with current node. If this list is empty, it is a leaf of the tree, and `fold`
/// will be run on this node.
///
/// ## `fold: FnMut(OutCtx, impl Iterator<Out>) -> impl Future<Output = Result<Out, Err>>`
/// Aynchronous function which given node context and output of `fold` for its chidlren
/// should produce new output value.
///
/// ## return value `impl Future<Output = Result<Out, Err>>`
/// Result of running fold operation on the root of the tree.
///
pub fn bounded_traversal<'caller, Err, In, Ins, Out, OutCtx, Unfold, Fold>(
    scheduled_max: usize,
    init: In,
    unfold: Unfold,
    fold: Fold,
) -> impl Future<Output = Result<Out, Err>> + 'caller
where
    Err: 'caller,
    Ins: 'caller,
    Out: 'caller,
    OutCtx: 'caller,
    // We use BoxFuture here because the `Unfold` future can be very large.
    // As a result, it's more efficient to keep it in one place (the heap)
    // than to move it around on the stack all the time.
    // https://fburl.com/m3cdcdko
    Unfold: FnMut(In) -> BoxFuture<'caller, Result<(OutCtx, Ins), Err>> + 'caller,
    Ins: IntoIterator<Item = In> + 'caller,
    Fold: FnMut(OutCtx, Iter<Out>) -> BoxFuture<'caller, Result<Out, Err>> + 'caller,
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
    UFut: Future,
    FFut: Future,
{
    unfold: Unfold,
    fold: Fold,
    scheduled_max: usize,
    scheduled: FuturesUnordered<Join<Ready<NodeLocation>, Either2<UFut, FFut>>>, // jobs being executed
    unscheduled: VecDeque<Join<Ready<NodeLocation>, Either2<UFut, FFut>>>, // as of yet unscheduled jobs
    execution_tree: HashMap<NodeIndex, Node<Out, OutCtx>>, // tree tracking execution process
    execution_tree_index: NodeIndex,                       // last allocated node index
}

impl<Err, In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut>
    BoundedTraversal<Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    Unfold: FnMut(In) -> UFut,
    UFut: Future<Output = Result<(OutCtx, Ins), Err>>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
    FFut: Future<Output = Result<Out, Err>>,
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
        let fut = join(ready(parent), Either2::Left((self.unfold)(value)));
        self.unscheduled.push_front(fut);
    }

    fn enqueue_fold(&mut self, parent: NodeLocation, context: OutCtx, children: Iter<Out>) {
        let fut = join(
            ready(parent),
            Either2::Right((self.fold)(context, children)),
        );
        self.unscheduled.push_front(fut);
    }

    fn process_unfold(&mut self, parent: NodeLocation, (context, children): (OutCtx, Ins)) {
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

impl<Err, In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut> Future
    for BoundedTraversal<Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    Unfold: FnMut(In) -> UFut,
    UFut: Future<Output = Result<(OutCtx, Ins), Err>>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
    FFut: Future<Output = Result<Out, Err>>,
{
    type Output = Result<Out, Err>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        loop {
            // schedule as many jobs as possible
            for job in this.unscheduled.drain(
                ..std::cmp::min(
                    this.unscheduled.len(),
                    this.scheduled_max - this.scheduled.len(),
                ),
            ) {
                this.scheduled.push(job);
            }

            // execute scheduled until it is blocked or done
            if let Some(job_result) = ready!(this.scheduled.poll_next_unpin(cx)) {
                match job_result {
                    (value, Either::Left(result)) => this.process_unfold(value, result?),
                    (value, Either::Right(result)) => {
                        // `0` is special index which means whole tree have been executed
                        if value.node_index == NodeIndex(0) {
                            // all jobs have to be completed and execution_tree empty
                            assert!(this.execution_tree.is_empty());
                            assert!(this.unscheduled.is_empty());
                            assert!(this.scheduled.is_empty());
                            return Poll::Ready(result);
                        }
                        this.process_fold(value, result?);
                    }
                }
            }
        }
    }
}
