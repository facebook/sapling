/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::common::Either2;
use super::common::NodeLocation;
use super::Iter;
use either::Either;
use futures::future::join;
use futures::future::ready;
use futures::future::BoxFuture;
use futures::future::Join;
use futures::future::Ready;
use futures::ready;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::hash::Hash;
use std::mem;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

/// `bounded_traversal_dag` traverses implicit asynchronous DAG specified by `init`
/// and `unfold` arguments, and it also does backward pass with `fold` operation.
/// All `unfold` and `fold` operations are executed in parallel if they do not
/// depend on each other (not related by ancestor-descendant relation in implicit DAG)
/// with amount of concurrency constrained by `scheduled_max`.
///
/// ## Difference between `bounded_traversal_dag` and `bounded_traversal`
/// Obvious difference is that `bounded_traversal_dag` correctly handles DAGs
/// (`bounded_traversal` treats all children references as distinct and its execution time
/// is proportional to number of paths from the root, since DAG can be constructed to contain
/// `O(exp(N))` path it might cause problems) but it comes with a price:
///  - `bounded_traversal_dag` keeps `Out` result of computation for all the nodes
///     but `bounded_traversal` only keeps results for nodes that have not been completely
///     evaluatated
///  - `In` has additional constraints to be `Eq + Hash + Clone`
///  - `Out` has additional constraint to be `Clone`
///
/// ## `init: In`
/// Is the root of the implicit tree to be traversed
///
/// ## `unfold: FnMut(In) -> impl Future<Output = Result<(OutCtx, impl IntoIterator<Item = In>), Err>>`
/// Asynchronous function which given input value produces list of its children. And context
/// associated with current node. If this list is empty, it is a leaf of the tree, and `fold`
/// will be run on this node.
///
/// ## `fold: FnMut(OutCtx, impl Iterator<Out>) -> impl Future<Item = Result<Out, Err>>`
/// Aynchronous function which given node context and output of `fold` for its chidlren
/// should produce new output value.
///
/// ## return value `impl Future<Output = Result<Option<Out>, Err>>`
/// Result of running fold operation on the root of the tree. `None` indiciate that cycle
/// has been found.
///
pub fn bounded_traversal_dag<'caller, Err, In, Ins, Out, OutCtx, Unfold, Fold>(
    scheduled_max: usize,
    init: In,
    unfold: Unfold,
    fold: Fold,
) -> impl Future<Output = Result<Option<Out>, Err>> + 'caller
where
    Err: 'caller,
    In: Eq + Hash + Clone + 'caller,
    Out: Clone + 'caller,
    OutCtx: 'caller,
    // We use BoxFuture here because the `Unfold` future can be very large.
    // As a result, it's more efficient to keep it in one place (the heap)
    // than to move it around on the stack all the time.
    // https://fburl.com/m3cdcdko
    Unfold: FnMut(In) -> BoxFuture<'caller, Result<(OutCtx, Ins), Err>> + 'caller,
    Ins: IntoIterator<Item = In> + 'caller,
    Fold: FnMut(OutCtx, Iter<Out>) -> BoxFuture<'caller, Result<Out, Err>> + 'caller,
{
    BoundedTraversalDAG::new(scheduled_max, init, unfold, fold)
}

struct Children<Out, OutCtx> {
    context: OutCtx,
    children: Vec<Option<Out>>,
    children_left: usize,
}

enum Node<In, Out, OutCtx> {
    Pending {
        parents: Vec<NodeLocation<In>>, // nodes blocked by current node
        children: Option<Children<Out, OutCtx>>, // present if node waits for children to be computed
    },
    Done(Out),
}

#[must_use = "futures do nothing unless polled"]
struct BoundedTraversalDAG<In, Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    UFut: Future,
    FFut: Future,
{
    init: In,
    unfold: Unfold,
    fold: Fold,
    scheduled_max: usize,
    scheduled: FuturesUnordered<Join<Ready<In>, Either2<UFut, FFut>>>, // jobs being executed
    unscheduled: VecDeque<Join<Ready<In>, Either2<UFut, FFut>>>,       // as of yet unscheduled jobs
    execution_tree: HashMap<In, Node<In, Out, OutCtx>>, // tree tracking execution process
}

impl<Err, In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut>
    BoundedTraversalDAG<In, Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    In: Clone + Eq + Hash,
    Out: Clone,
    Unfold: FnMut(In) -> UFut,
    UFut: Future<Output = Result<(OutCtx, Ins), Err>>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
    FFut: Future<Output = Result<Out, Err>>,
{
    fn new(scheduled_max: usize, init: In, unfold: Unfold, fold: Fold) -> Self {
        let mut this = Self {
            init: init.clone(),
            unfold,
            fold,
            scheduled_max,
            scheduled: FuturesUnordered::new(),
            unscheduled: VecDeque::new(),
            execution_tree: HashMap::new(),
        };
        let init_out = this.enqueue_unfold(
            NodeLocation {
                node_index: init.clone(),
                child_index: 0,
            },
            init,
        );
        // can not be resolved since execution tree is empty
        debug_assert!(init_out.is_none());
        this
    }

    fn enqueue_unfold(&mut self, parent: NodeLocation<In>, value: In) -> Option<Out> {
        match self.execution_tree.get_mut(&value) {
            None => {
                // schedule unfold for previously unseen `value`
                self.execution_tree.insert(
                    value.clone(),
                    Node::Pending {
                        parents: vec![parent],
                        children: None,
                    },
                );
                self.unscheduled.push_front(join(
                    ready(value.clone()),
                    Either2::Left((self.unfold)(value)),
                ));
                None
            }
            Some(Node::Pending { parents, .. }) => {
                // we already have a node associated with the same input value,
                // register as a dependency for this node.
                parents.push(parent);
                None
            }
            Some(Node::Done(result)) => Some(result.clone()),
        }
    }

    fn enqueue_fold(&mut self, value: In, context: OutCtx, children: Iter<Out>) {
        self.unscheduled.push_front(join(
            ready(value),
            Either2::Right((self.fold)(context, children)),
        ));
    }

    fn process_unfold(&mut self, value: In, (context, children): (OutCtx, Ins)) {
        // schedule unfold for node's children
        let mut children_left = 0;
        let children: Vec<_> = children
            .into_iter()
            .enumerate()
            .map(|(child_index, child)| {
                let out = self.enqueue_unfold(
                    NodeLocation {
                        node_index: value.clone(),
                        child_index,
                    },
                    child,
                );
                if out.is_none() {
                    children_left += 1;
                }
                out
            })
            .collect();

        if children_left != 0 {
            // update pending node with `wait` state
            let node = self
                .execution_tree
                .get_mut(&value)
                .expect("unfold referenced invalid node");
            match node {
                Node::Pending { children: wait, .. } => {
                    *wait = Some(Children {
                        context,
                        children,
                        children_left,
                    });
                }
                _ => unreachable!("running unfold for Node::Done"),
            }
        } else {
            // do not have any dependencies (leaf node), schedule fold immediately
            self.enqueue_fold(value, context, children.into_iter().flatten());
        }
    }

    fn process_fold(&mut self, value: In, result: Out) {
        // mark node as done
        let node = self
            .execution_tree
            .get_mut(&value)
            .expect("fold referenced invalid node");
        let parents = match mem::replace(node, Node::Done(result.clone())) {
            Node::Pending { parents, .. } => parents,
            _ => unreachable!("running fold for Node::Done"),
        };

        // update all the parents wait for this result
        for parent in parents {
            self.update_location(parent, result.clone());
        }
    }

    fn update_location(&mut self, loc: NodeLocation<In>, result: Out) {
        let node = self
            .execution_tree
            .get_mut(&loc.node_index)
            .expect("`update_location` referenced invalid node");
        let children = match node {
            Node::Pending { children, .. } => children,
            _ => unreachable!("updating already resolved parent node"),
        };
        let no_children_left = {
            // update parent
            let mut children = children
                .as_mut()
                .expect("`update_location` referenced not blocked node");
            debug_assert!(children.children[loc.child_index].is_none());
            children.children[loc.child_index] = Some(result);
            children.children_left -= 1;
            children.children_left == 0
        };
        if no_children_left {
            // all parents children have been completed, so we need
            // to schedule fold operation for it
            let Children {
                context, children, ..
            } = children
                .take()
                .expect("`update_location` reference node without children");
            self.enqueue_fold(loc.node_index, context, children.into_iter().flatten());
        }
    }
}

impl<Err, In, Ins, Out, OutCtx, Unfold, UFut, Fold, FFut> Future
    for BoundedTraversalDAG<In, Out, OutCtx, Unfold, UFut, Fold, FFut>
where
    In: Eq + Hash + Clone,
    Out: Clone,
    Unfold: FnMut(In) -> UFut,
    UFut: Future<Output = Result<(OutCtx, Ins), Err>>,
    Ins: IntoIterator<Item = In>,
    Fold: FnMut(OutCtx, Iter<Out>) -> FFut,
    FFut: Future<Output = Result<Out, Err>>,
{
    type Output = Result<Option<Out>, Err>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        loop {
            if this.unscheduled.is_empty() && this.scheduled.is_empty() {
                // we have not received result of with `value == init` and
                // nothing is scheduled or unscheduled, it means that we have
                // cycle dependency somewhere inside input graph
                return Poll::Ready(Ok(None));
            }

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
                        // we have computed value associated with `init` node
                        if value == this.init {
                            // all jobs have to be completed and execution_tree empty
                            assert!(this.unscheduled.is_empty());
                            assert!(this.scheduled.is_empty());
                            return Poll::Ready(Ok(Some(result?)));
                        }
                        this.process_fold(value, result?);
                    }
                }
            }
        }
    }
}
