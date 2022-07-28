/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures::future::join;
use futures::future::ready;
use futures::future::BoxFuture;
use futures::future::Join;
use futures::future::Ready;
use futures::ready;
use futures::stream::FuturesUnordered;
use futures::stream::Stream;
use futures::stream::StreamExt;
use smallvec::SmallVec;

use super::common::OrderedTraversal;
use super::error::BoundedTraversalError;

/// Index of an executing node in the queue of nodes.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
struct NodeIndex(usize);

/// Location of a node relative to its parent (parent node index and index
/// within the parent's list of children)).
type NodeLocation = super::common::NodeLocation<NodeIndex>;

/// Execution Tree Node
struct Node<Out, In> {
    /// The location of this node.
    parent: Option<NodeLocation>,

    /// The children of this node.
    children: Vec<Child<Out, In>>,

    /// The remaining weight of this node.  This is the budget required to complete
    /// the remaining work for this node.  Weight is subtracted when immediate
    /// children are yielded, or when budget is moved into a child node.
    remaining_weight: usize,

    /// The budget allocated to this node from the overall queuing budget.
    /// This is used to schedule work from this node for only the child
    /// entries that fit within the budget.
    budget: usize,

    /// The budget allocated to this node which has expired because the limit
    /// has been reached.  This will be cancelled when this node completes.
    expired: usize,

    /// The next child index that needs to be considered for scheduling.
    schedule_next_index: usize,
}

/// Child of Execution Tree Node
enum Child<Out, In> {
    /// This child has not been scheduled yet.
    Unscheduled { weight: usize, input: In },

    /// This child has been scheduled, and will be filled in by a future that
    /// has yet to complete.
    Pending { weight: usize },

    /// This child is a value to yield.
    Output(Out),

    /// This child is another node.
    Node {
        weight: usize,
        node_index: NodeIndex,
    },

    /// This child has been yielded from the stream.
    Yielded,
}

impl<Out, In> Child<Out, In> {
    /// Returns the weight of the child.  This is the budget that is consumed
    /// within this node for this chid.
    fn weight(&self) -> usize {
        match self {
            Child::Unscheduled { weight, .. } => *weight,
            Child::Pending { weight, .. } => *weight,
            Child::Node { weight, .. } => *weight,
            Child::Output(_) => 1,
            Child::Yielded => 0,
        }
    }
}

/// State for scheduling the children of a node.
struct NodeSchedule {
    /// The index of the next child to process.
    next_child_index: usize,

    /// The remaining budget for this schedule.
    budget: usize,

    /// Count of pending nodes that have been encountered.
    pending_count: usize,
}

/// A scheduling action to perform.
enum NodeAction<In> {
    /// This input should be scheduled for unfolding.
    Schedule { child_index: usize, input: In },

    /// A child node should be recursed into with the given additional budget.
    Recurse {
        child_index: usize,
        node_index: NodeIndex,
        budget: usize,
    },

    /// All children have been scheduled and some are still pending.
    Pending,

    /// All children have been scheduled and have completed.
    Complete,
}

/// Values yielded from nodes.
enum NodeYield<Out> {
    /// This node yields output.
    Output { child_index: usize, output: Out },

    /// A child node may yield output.
    FromChild { node_index: NodeIndex },

    /// The next output item is still pending.
    Pending { child_index: usize },

    /// This node has yielded all of its values.
    Complete,
}

/// Values yielded from node traversal.
enum Yield<Out> {
    /// An output item is yielded.  If `None` is output then the stream is
    /// complete.
    Output(Option<Out>),

    /// The next output item is still pending.
    Pending,
}

impl<Out, In> Node<Out, In> {
    fn new(parent: Option<NodeLocation>, children: Vec<Child<Out, In>>, weight: usize) -> Self {
        Node {
            parent,
            children,
            remaining_weight: weight,
            budget: 0,
            expired: 0,
            schedule_next_index: 0,
        }
    }

    /// Add `budget` to this node's budget.  Any surplus is not added to the
    /// budget, and is instead returned.
    fn add_budget(&mut self, budget: usize) -> usize {
        let total = self.budget + budget;
        self.budget = total.min(self.remaining_weight);
        total - self.budget
    }

    /// Mark `budget` from this node's budget as expired.
    fn expire_budget(&mut self, budget: usize) {
        self.expired += budget;
    }

    /// Take any expired budget that is covered by the surplus.
    fn take_expired(&mut self) -> usize {
        let surplus = self.budget.saturating_sub(self.remaining_weight);
        let expired = self.expired.min(surplus);
        self.budget -= expired;
        self.expired -= expired;
        expired
    }

    /// Take any surplus budget from this node which can be returned to the
    /// parent.
    fn take_surplus(&mut self) -> usize {
        let surplus = self.budget.saturating_sub(self.remaining_weight);
        self.budget -= surplus;
        surplus
    }

    /// The amount of budget this node still needs.
    fn budget_needed(&self) -> usize {
        self.remaining_weight.saturating_sub(self.budget)
    }

    /// Create a `NodeSchedule` to schedule the children of this node.
    fn schedule(&self) -> NodeSchedule {
        NodeSchedule {
            next_child_index: self.schedule_next_index,
            budget: self.budget,
            pending_count: 0,
        }
    }

    /// Yield the next value from this node, starting at `child_index`.
    fn yield_next(&mut self, mut child_index: usize) -> NodeYield<Out> {
        while child_index < self.children.len() {
            let child = &mut self.children[child_index];
            match child {
                Child::Unscheduled { .. } | Child::Pending { .. } => {
                    // This child is not ready to yield.
                    return NodeYield::Pending { child_index };
                }
                Child::Output(_) => {
                    // This child is ready to yield.  Yield it if we have
                    // budget to do so.
                    if self.budget == 0 {
                        return NodeYield::Pending { child_index };
                    }
                    if let Child::Output(output) = std::mem::replace(child, Child::Yielded) {
                        // Remove the child's weight from the node as it is
                        // now yielded.
                        self.remaining_weight -= 1;
                        return NodeYield::Output {
                            child_index,
                            output,
                        };
                    }
                }
                Child::Node { node_index, .. } => {
                    // This child node is ready to be yielded from.
                    return NodeYield::FromChild {
                        node_index: *node_index,
                    };
                }
                Child::Yielded => {}
            }
            child_index += 1;
        }
        NodeYield::Complete
    }

    /// Called when a child node yields a value.  If the child node had weight
    /// that was pending a budget allocation that is more than the child now
    /// needs, the weight can be reduced.
    fn child_yielded(&mut self, child_index: usize, child_budget_needed: usize) {
        if let Child::Node { weight, .. } = &mut self.children[child_index] {
            if *weight > child_budget_needed {
                let cancelled = *weight - child_budget_needed;
                *weight -= cancelled;
                self.remaining_weight -= cancelled;
            }
        }
    }

    /// Called when a child node has yielded all of its values.
    fn child_complete(&mut self, child_index: usize, surplus: usize) {
        //debug_assert_eq!(self.children[child_index].weight(), 0);
        let child_weight = self.children[child_index].weight();
        self.children[child_index] = Child::Yielded;
        // Add the child's surplus to this node's budget.
        self.budget += surplus;
        self.remaining_weight -= child_weight;
    }
}

impl NodeSchedule {
    /// Attempt to consume `budget` from this scheduling operation.   Returns
    /// the amount that was actually consumed.
    fn consume_budget(&mut self, mut budget: usize) -> usize {
        budget = budget.min(self.budget);
        self.budget -= budget;
        budget
    }

    /// Get the next scheduling action for this node.
    fn next_action<Out, In>(&mut self, node: &mut Node<Out, In>) -> NodeAction<In> {
        while self.next_child_index < node.children.len() {
            let child_index = self.next_child_index;
            self.next_child_index += 1;
            let child_weight = node.children[child_index].weight();

            // Attempt to consume this child's weight from the budget.
            let budget = self.consume_budget(child_weight);

            let child = &mut node.children[child_index];
            match child {
                Child::Unscheduled { weight, .. } => {
                    // This is an unscheduled child input waiting to be
                    // scheduled.
                    self.pending_count += 1;
                    if budget > 0 {
                        // There is budget available to schedule this child.
                        let weight = *weight;
                        if let Child::Unscheduled { input, .. } =
                            std::mem::replace(child, Child::Pending { weight })
                        {
                            return NodeAction::Schedule { child_index, input };
                        }
                    }
                }
                Child::Node { weight, node_index } => {
                    // This is a child node that must be recursed into.
                    self.pending_count += 1;
                    if budget > 0 {
                        // There is additional budget to move into the child
                        *weight -= budget;
                        node.remaining_weight -= budget;
                        node.budget -= budget;
                    }
                    return NodeAction::Recurse {
                        child_index,
                        node_index: *node_index,
                        budget,
                    };
                }
                Child::Pending { .. } => {
                    // This child is pending an executing future.
                    self.pending_count += 1;
                }
                Child::Yielded => {
                    // This child has completed and does not need to be
                    // scheduled.
                    if child_index == node.schedule_next_index {
                        // This child can be skipped next time round.
                        node.schedule_next_index += 1;
                    }
                }
                Child::Output(..) => {}
            }
        }
        // If any nodes were pending, this node is not complete and will need
        // to be scheduled again.
        if self.pending_count > 0 {
            NodeAction::Pending
        } else {
            NodeAction::Complete
        }
    }

    // Called when a child node's scheduling is complete.
    fn child_scheduled(&mut self, _child_index: usize) {
        // The child is no longer pending.
        self.pending_count -= 1;
    }
}

/// Stream implementation for bounded traversal of a tree as a stream with
/// ordered output.
///
/// See `bounded_traversal_ordered_stream` and
/// `bounded_traversal_limited_ordered_stream` for constructing a
/// `BoundedTraversalOrderedStream`.
///
/// The implementation works with two parts: the execution tree, and the
/// scheduled futures.
///
/// The execution tree is a tree of `Node`s, representing an unfolded
/// recursion item in the tree.  The order of the Node's children are
/// maintained, and values are yielded from them in order by the `yield_next`
/// method.
///
/// Selection of whether a node child that is a recursion item
/// (`Child::Unscheduled`) is scheduled for unfolding is done by the
/// `schedule_next` method.  This uses a budget, which is taken from the
/// `schedule_max` parameter.  Nodes can only be unfolded if there is suitable
/// queue budget available for them and they are the next nodes in the ordered
/// traversal of the tree.
///
/// Within a node, the node budget covers:
/// * Unyielded child output items that are queued behind other items that
///   have not yet been yielded.
/// * Child recursion nodes that are currently unfolding.
/// * Unfolded child nodes that require more budget.  Budget is transferred
///   from the parent node to the child node so that the child can unfold
///   its childrent and yield its node.
///
/// Once a node has unfolded enough items, it returns its surplus budget to its
/// parent node.
///
/// When a node is selected for unfolding, the unfold callback is called, and
/// the returned future is passed to the scheduled futures.  While the future
/// is outstanding, the child is marked as `Child::Pending`.
///
/// The scheduled futures are run concurrently (via `FuturesUnordered`).  The
/// unfold futures may complete in any order.  Unfolds that are *after*
/// earlier unfolds in the output order will have their values queued until
/// the earlier unfolds' values have been yielded.
///
/// If more than `scheduled_max` futures are outstanding, the excess futures
/// will be queued.
///
/// When a future completes, its entry in the execution tree is looked up via
/// its `NodeLocation` and is then replaced with a new unfolded node, and the
/// execution tree is re-evaluated for the next step.  If there is sufficient
/// budget in the parent node then the newly unfolded node will be scheduled.
struct BoundedTraversalOrderedStream<Out, In, Unfold, UFut>
where
    UFut: Future,
{
    unfold: Unfold,
    scheduled_max: usize,
    total_budget: usize,
    limit: Option<usize>,
    schedule_queue: VecDeque<Join<Ready<NodeLocation>, UFut>>,
    scheduled: FuturesUnordered<Join<Ready<NodeLocation>, UFut>>,
    execution_tree: HashMap<NodeIndex, Node<Out, In>>,
    execution_tree_index: NodeIndex,
    yield_next_location: Option<NodeLocation>,
}

impl<Out, In, Unfold, UFut, Unfolded, TErr> BoundedTraversalOrderedStream<Out, In, Unfold, UFut>
where
    Unfold: FnMut(In) -> UFut,
    UFut: Future<Output = Result<Unfolded, TErr>>,
    Unfolded: IntoIterator<Item = OrderedTraversal<Out, In>>,
    TErr: From<BoundedTraversalError>,
{
    /// Construct a new `BoundedTraversalOrderedStream`.
    ///
    /// Initially there is a single root node which has all of the input nodes
    /// as children, and the whole budget.
    fn new<InsInit>(
        scheduled_max: NonZeroUsize,
        queued_max: NonZeroUsize,
        limit: Option<usize>,
        init: InsInit,
        unfold: Unfold,
    ) -> Self
    where
        InsInit: IntoIterator<Item = (usize, In)>,
    {
        let scheduled_max = scheduled_max.get();
        let queued_max = queued_max.get();
        let schedule_queue = VecDeque::new();
        let scheduled = FuturesUnordered::new();
        let mut children = Vec::new();
        let mut total_weight = 0;
        for (weight, input) in init {
            let weight = weight.max(1);
            total_weight += weight;
            children.push(Child::Unscheduled { weight, input });
        }

        let mut root = Node::new(None, children, total_weight);
        let total_budget = queued_max - root.add_budget(queued_max);
        let execution_tree_index = NodeIndex(0);
        let mut execution_tree = HashMap::new();
        execution_tree.insert(execution_tree_index, root);
        // If limit is zero then start in the completely yielded state.
        let yield_next_location = if limit == Some(0) {
            None
        } else {
            Some(NodeLocation::new(execution_tree_index, 0))
        };
        BoundedTraversalOrderedStream {
            unfold,
            scheduled_max,
            total_budget,
            limit,
            schedule_queue,
            scheduled,
            execution_tree,
            execution_tree_index,
            yield_next_location,
        }
    }

    /// Clears the execution tree and scheduled futures.  Called once the
    /// output limit is reached to cancel all further work.
    fn clear(&mut self) {
        self.yield_next_location = None;
        self.total_budget = 0;
        self.execution_tree.clear();
        self.schedule_queue.clear();
        self.scheduled = FuturesUnordered::new();
    }

    /// Determine the next items to be scheduled.
    ///
    /// Iterates over the portion of the execution tree that contains nodes
    /// that may need scheduling, and schedules their unscheduled children if
    /// the available budget allows.
    fn schedule_next(&mut self) -> Result<(), BoundedTraversalError> {
        /// Stack frame for iterative recursion.
        struct Frame {
            /// The node index currently being considered for scheduling.
            node_index: NodeIndex,

            /// The child index of this node within its parent.  `None` for
            /// the root node.
            child_index: Option<usize>,

            /// The state for this node's scheduling.
            node_schedule: Option<NodeSchedule>,

            /// The surplus budget that should be released to this node's
            /// parent.
            surplus: usize,
        }

        // Stack for iterative recursion.  This is a SmallVec so that it
        // doesn't normally require an allocation unless the tree is
        // particularly deep.  32 stack frames are currently around 2KB.
        let mut stack = SmallVec::<[Frame; 32]>::new();

        // Set to a child index when a node completes and control is passed
        // back to its parent.
        let mut completed_child_index = None;

        // Additional available budget to be added to the next node.
        let mut additional_budget = 0;

        stack.push(Frame {
            node_index: NodeIndex(0),
            child_index: None,
            node_schedule: None,
            surplus: 0,
        });

        while let Some(Frame {
            node_index,
            child_index,
            ref mut node_schedule,
            ref mut surplus,
        }) = stack.last_mut()
        {
            let node = self
                .execution_tree
                .get_mut(node_index)
                .ok_or_else(|| programming_error!("node must exist at {:?}", node_index))?;

            let node_schedule = node_schedule.get_or_insert_with(|| node.schedule());

            // If the child we just processed completed, mark the child as
            // fully scheduled.
            if let Some(child_index) = completed_child_index.take() {
                node_schedule.child_scheduled(child_index);
            }

            // Add the available budget to the node, and store any surplus for
            // return to the parent.
            *surplus += node.add_budget(additional_budget);

            loop {
                let action = node_schedule.next_action(node);
                match action {
                    NodeAction::Schedule { child_index, input } => {
                        // This child is ready to schedule.  Construct a
                        // future to unfold it and schedule it for execution.
                        let location = NodeLocation::new(*node_index, child_index);
                        let unfold_fut = join(ready(location), (self.unfold)(input));
                        if self.scheduled.len() >= self.scheduled_max {
                            self.schedule_queue.push_back(unfold_fut);
                        } else {
                            self.scheduled.push(unfold_fut);
                        }
                    }
                    NodeAction::Recurse {
                        child_index,
                        node_index,
                        budget,
                    } => {
                        // This child node must be recursively examined for
                        // scheduling.
                        additional_budget = budget;
                        stack.push(Frame {
                            node_index,
                            child_index: Some(child_index),
                            node_schedule: None,
                            surplus: 0,
                        });
                        break;
                    }
                    NodeAction::Pending => {
                        // This node has finished scheduling for now, but
                        // still has pending children.  Pass its surplus to
                        // its parent.
                        additional_budget = *surplus;
                        stack.pop();
                        break;
                    }
                    NodeAction::Complete => {
                        // This node has finished scheduling, and all of its
                        // children are also complete.  Pass its surplus to
                        // its parent and mark it as scheduled.
                        additional_budget = *surplus;
                        completed_child_index = *child_index;
                        stack.pop();
                        break;
                    }
                }
            }
        }

        // Remove any remaining additional budget from the total budget.
        self.total_budget -= additional_budget;

        Ok(())
    }

    /// Determine the next item to be yielded from the stream, if it is
    /// available.
    fn yield_next(&mut self) -> Result<Yield<Out>, BoundedTraversalError> {
        // The location we should start from is stored in
        // `yield_next_location`.
        let mut location = self.yield_next_location.take();

        // Set to the surplus budget when a child node is completed.  The
        // surplus budget is passed back to the parent node.
        loop {
            let NodeLocation {
                node_index,
                child_index,
            } = match location {
                None => return Ok(Yield::Output(None)),
                Some(location) => location,
            };

            // The entry should always exist unless `clear` has been called
            // because we are terminating early due to reaching the limit.
            let mut entry = match self.execution_tree.entry(node_index) {
                Entry::Vacant(_) => return Ok(Yield::Output(None)),
                Entry::Occupied(entry) => entry,
            };
            let mut node = entry.get_mut();

            match node.yield_next(child_index) {
                NodeYield::FromChild { node_index } => {
                    // A child node should be examined for items to yield.
                    location = Some(NodeLocation::new(node_index, 0));
                }
                NodeYield::Complete => {
                    // All values from this node have been yielded.  Move to
                    // the node's parent, return its surplus to that parent,
                    // and remove the node from the execution tree.
                    let node = entry.remove();
                    debug_assert_eq!(node.remaining_weight, 0);
                    if let Some(parent) = node.parent {
                        if let Some(parent_node) = self.execution_tree.get_mut(&parent.node_index) {
                            parent_node.child_complete(parent.child_index, node.budget);
                        }
                    }
                    location = node.parent;
                }
                NodeYield::Pending { child_index } => {
                    // The item in `child_index` is the next to be yielded,
                    // but it is not ready yet.  Store this location so that
                    // we can continue here next time.
                    self.yield_next_location = Some(NodeLocation::new(node_index, child_index));
                    return Ok(Yield::Pending);
                }
                NodeYield::Output {
                    child_index,
                    output,
                } => {
                    // The item in `child_index` has the value `output` and is
                    // ready to be yielded.
                    if let Some(ref mut limit) = self.limit {
                        // If there is a limit, yielding this item reduces it
                        // by one.
                        *limit -= 1;
                        if *limit < self.total_budget {
                            // If the limit was already below the queued max,
                            // reduce the budget so that we don't schedule
                            // futures for items that will not be returned.
                            node.expire_budget(1);
                        }
                    }

                    if self.limit == Some(0) {
                        // If we have reached the limit, clear all pending
                        // operations and terminate the stream.
                        self.clear();
                        self.yield_next_location = None;
                    } else {
                        // Record the next child at the next place to yield
                        // from.
                        self.yield_next_location =
                            Some(NodeLocation::new(node_index, child_index + 1));

                        // Take this node's surplus for expiration and
                        // returning to the parent node.
                        self.total_budget -= node.take_expired();
                        let mut surplus = node.take_surplus();
                        let mut budget_needed = node.budget_needed();

                        // Walk up the execution tree recording that each node's
                        // child has yielded a value, and passing the surplus up.
                        while let Some(parent_location) = node.parent {
                            node = self
                                .execution_tree
                                .get_mut(&parent_location.node_index)
                                .ok_or_else(|| {
                                    programming_error!(
                                        "parent must exist at {:?}",
                                        parent_location.node_index,
                                    )
                                })?;
                            node.child_yielded(parent_location.child_index, budget_needed);
                            surplus = node.add_budget(surplus);
                            budget_needed = node.budget_needed();
                        }
                        self.total_budget -= surplus;
                    }

                    return Ok(Yield::Output(Some(output)));
                }
            }
        }
    }

    /// Process a completed unfold by inserting a new node into the execution
    /// tree.
    fn process_unfold(
        &mut self,
        location: NodeLocation,
        unfolded: Unfolded,
    ) -> Result<(), BoundedTraversalError> {
        // Allocate node_index
        self.execution_tree_index = NodeIndex(self.execution_tree_index.0 + 1);
        let node_index = self.execution_tree_index;

        // Create the new node's children.
        let mut children = Vec::new();
        let mut total_weight = 0;
        for item in unfolded {
            match item {
                OrderedTraversal::Output(out) => {
                    total_weight += 1;
                    children.push(Child::Output(out));
                }
                OrderedTraversal::Recurse(weight, input) => {
                    let weight = weight.max(1);
                    total_weight += weight;
                    children.push(Child::Unscheduled { weight, input });
                }
            }
        }

        let parent = self
            .execution_tree
            .get_mut(&location.node_index)
            .ok_or_else(|| {
                programming_error!(
                    "after unfold, parent must exist at {:?}",
                    location.node_index,
                )
            })?;
        let original_weight = parent.children[location.child_index].weight();

        if children.is_empty() {
            // This item unfolded into nothing.  Treat it as if it completely
            // yielded.
            parent.children[location.child_index] = Child::Yielded;
            parent.remaining_weight -= original_weight;
        } else {
            // The child's new weight is the minimum of what was requested and
            // what the child actually needs, i.e. it may be less than the
            // child's actual weight if the estimate was too low.  This will
            // be the budget that the child receives (once enough is
            // available).  The child will still be able to make progress with
            // a lower budget, but may be slower if the estimate was very
            // wrong.
            let child_weight = total_weight.min(original_weight);

            // The released budget is the unused portion of the original
            // weight.  Move weight to the child to release the additional
            // budget.
            let released_budget = original_weight - child_weight;
            parent.remaining_weight -= released_budget;

            // Replace the parent's child entry with the new node's index.
            parent.children[location.child_index] = Child::Node {
                weight: child_weight,
                node_index,
            };

            // Store the new node in the execution tree.
            let node = Node::new(Some(location), children, total_weight);
            self.execution_tree.insert(node_index, node);
        }

        Ok(())
    }

    #[cfg(test)]
    fn check_budget(&self) {
        let total_budget = self
            .execution_tree
            .values()
            .map(|node| node.budget)
            .sum::<usize>();
        assert_eq!(total_budget, self.total_budget);
    }
}

impl<Out, In, Unfold, UFut, Unfolded, TErr> Stream
    for BoundedTraversalOrderedStream<Out, In, Unfold, UFut>
where
    Unfold: FnMut(In) -> UFut,
    UFut: Future<Output = Result<Unfolded, TErr>>,
    Unfolded: IntoIterator<Item = OrderedTraversal<Out, In>>,
    TErr: From<BoundedTraversalError>,
{
    type Item = Result<Out, TErr>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        loop {
            // Yield any values that are ready to be yielded.  This will
            // release budget.
            if let Yield::Output(output) = this.yield_next()? {
                return Poll::Ready(output.map(Ok));
            }

            // Schedule any unfold futures that now have sufficient budget to
            // proceed.
            this.schedule_next()?;

            // In tests, check that the budget is balanced.
            #[cfg(test)]
            this.check_budget();

            // Yield any values that are ready to be yielded now that budget
            // has been reassigned.
            if let Yield::Output(output) = this.yield_next()? {
                return Poll::Ready(output.map(Ok));
            }

            // There is nothing left to yield.  Wait for a scheduled unfold to
            // complete.
            if let Some((location, result)) = ready!(this.scheduled.poll_next_unpin(cx)) {
                if let Some(next_job) = this.schedule_queue.pop_front() {
                    this.scheduled.push(next_job);
                }
                this.process_unfold(location, result?)?;
            }
        }
    }
}

/// `bounded_traversal_ordered_stream` traverses the implicit asynchronous
/// tree specified by the `init` and `unfold` arguments.  All `unfold`
/// operations are executed in parallel if they do not depend on each other.
/// Unlike `bounded_traversal_stream`, the order of items in the traversal
/// is maintained.
///
/// There are two limiting factors for the concurrency level of ordered
/// traversal: `queued_max` and `scheduled_max`.
///
/// * `scheduled_max`: Limits the number of concurrent unfold futures that are
///                    run.  If there are more unfolds needed then they will
///                    be queued.
///
/// * `queued_max`: Limits the number of stream values that can be queued.
///                 Once the limit is reached, no more unfolds will be
///                 scheduled until enough values have been yielded to reduce
///                 the queue below this limit.  Note that the queue might
///                 still grow larger than this limit if already-running
///                 unfolds yield more items.
///
/// * `init`: The root(s) of the implicit tree to be traversed and their total
///           weights, which are the estimated numbers of items each tree
///           contains.
///
/// * `unfold`: A callback that returns a try-future that unfolds a single
///             input item into a sequence of output or recursion items.
///             Unlike the unordered `bounded_traversal_stream`, the ordering
///             between the items in the stream is maintained, with the
///             results of unfolding recursion items taking the place of
///             each recursion item in the stream.
///
/// The roots and recursion items must include an estimate of the weight of
/// the tree or subtree, this being the total number of items that it is
/// expected to yield.  The estimate should be as accurate as possible, and
/// where complete accuracy is not possible, it should favour over-estimation
/// rather than under-estimation.
///
/// If the estimate is too large, then more work will be queued behind the
/// unfold of the recursion item than is strictly necessary.  Once the unfold
/// is complete and the size of its elements are known, the excess queued work
/// will be scheduled.
///
/// If the estimate is too small, the child item will only have the same
/// concurrency budget as the estimate, which may be less than is truly
/// available.  Depending on the degree of shortfall, this may cause it to be
/// traversed significantly more slowly.
///
/// Returns a stream of all `Out` values in the order of the traversed tree.
pub fn bounded_traversal_ordered_stream<'caller, In, InsInit, Out, Unfold, Unfolded, TErr>(
    scheduled_max: NonZeroUsize,
    queued_max: NonZeroUsize,
    init: InsInit,
    unfold: Unfold,
) -> impl Stream<Item = Result<Out, TErr>> + 'caller
where
    In: 'caller,
    Out: 'caller,
    // We use BoxFuture here because the `Unfold` future can be very large.
    // As a result, it's more efficient to keep it in one place (the heap)
    // than to move it around on the stack all the time.
    // https://fburl.com/m3cdcdko
    Unfold: FnMut(In) -> BoxFuture<'caller, Result<Unfolded, TErr>> + 'caller,
    InsInit: IntoIterator<Item = (usize, In)> + 'caller,
    Unfolded: IntoIterator<Item = OrderedTraversal<Out, In>> + 'caller,
    TErr: From<BoundedTraversalError> + 'caller,
{
    BoundedTraversalOrderedStream::new(scheduled_max, queued_max, None, init, unfold)
}

/// Like `bounded_traversal_ordered_stream` with one additional parameter:
///
/// * `limit`: Specifies the limit to the number of output items.  This is
///            similar to calling the `StreamExt::take` method on the output
///            stream but attempts to avoid doing unnecessary unfold calls as
///            the limit is approached.
///
/// Use of this method over `StreamExt::take` works best when the estimate for
/// the number of items subtrees will yield is accurate.  If the difference is
/// large, it may be best to use `StreamExt::take` and accept that unnecessary
/// work may occur.
///
/// See `bounded_traversal_ordered_stream` for documentation of the remaining
/// parameters.
pub fn bounded_traversal_limited_ordered_stream<'caller, In, InsInit, Out, Unfold, Unfolded, TErr>(
    scheduled_max: NonZeroUsize,
    queued_max: NonZeroUsize,
    limit: usize,
    init: InsInit,
    unfold: Unfold,
) -> impl Stream<Item = Result<Out, TErr>> + 'caller
where
    In: 'caller,
    Out: 'caller,
    // We use BoxFuture here because the `Unfold` future can be very large.
    // As a result, it's more efficient to keep it in one place (the heap)
    // than to move it around on the stack all the time.
    // https://fburl.com/m3cdcdko
    Unfold: FnMut(In) -> BoxFuture<'caller, Result<Unfolded, TErr>> + 'caller,
    InsInit: IntoIterator<Item = (usize, In)> + 'caller,
    Unfolded: IntoIterator<Item = OrderedTraversal<Out, In>> + 'caller,
    TErr: From<BoundedTraversalError> + 'caller,
{
    BoundedTraversalOrderedStream::new(scheduled_max, queued_max, Some(limit), init, unfold)
}
