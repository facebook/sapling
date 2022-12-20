/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;
use std::iter::Peekable;

use anyhow::Error;
use borrowed::borrowed;
use bounded_traversal::OrderedTraversal;
use context::CoreContext;
use futures::future;
use futures::future::FutureExt;
use futures::pin_mut;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use nonzero_ext::nonzero;

use crate::ops::Diff;
use crate::select::select_path_tree;
use crate::AsyncManifest as Manifest;
use crate::AsyncOrderedManifest as OrderedManifest;
use crate::Entry;
use crate::PathOrPrefix;
use crate::PathTree;
use crate::StoreLoadable;

/// Track where we are relative to the `after` parameter.
pub enum After {
    /// Include everything.
    All,

    /// Include all contents, but omit the directory itself.
    AllContents,

    /// Include everything in this directory after the named element and the
    /// subpath within that element.
    After(MPathElement, Option<MPath>),
}

impl From<Option<Option<MPath>>> for After {
    fn from(path: Option<Option<MPath>>) -> Self {
        path.map_or(After::All, |p| After::new(p.as_ref()))
    }
}

impl After {
    fn new(mpath_opt: Option<&MPath>) -> Self {
        match mpath_opt {
            None => After::AllContents,
            Some(mpath) => {
                let (elem, rest) = mpath.split_first();
                After::After(elem.clone(), rest)
            }
        }
    }

    /// Returns true if this element should be skipped entirely.
    ///
    /// We don't skip entries that match exactly, even though they themselves
    /// will not be included.  If the element name matches then we still want
    /// to descend into subdirectories.
    pub fn skip(&self, name: &MPathElement) -> bool {
        match self {
            After::All | After::AllContents => false,
            After::After(elem, _) => name < elem,
        }
    }

    /// Returns true if this directory itself should be included.
    pub fn include_self(&self) -> bool {
        match self {
            After::All => true,
            After::AllContents | After::After(..) => false,
        }
    }

    /// Returns true if a file with the given name in this directory should be
    /// included.
    pub fn include_file(&self, name: &MPathElement) -> bool {
        match self {
            After::All | After::AllContents => true,
            After::After(elem, _) => name > elem,
        }
    }

    /// Enter a subdirectory.  The directory must be one that should be
    /// entered (i.e. skip is false).  Returns an instance of `After` suitable
    /// for the subdirectory.
    pub fn enter_dir(&self, name: &MPathElement) -> After {
        match self {
            After::All | After::AllContents => After::All,
            After::After(elem, rest) => {
                if name == elem {
                    After::new(rest.as_ref())
                } else {
                    debug_assert!(name > elem);
                    After::All
                }
            }
        }
    }
}

pub trait ManifestOrderedOps<Store>
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = Self> + OrderedManifest<Store> + Send + Sync,
    <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId: Clone + Send + Eq + Unpin,
{
    fn find_entries_ordered<I, P>(
        &self,
        ctx: CoreContext,
        store: Store,
        paths_or_prefixes: I,
        after: impl Into<After>,
    ) -> BoxStream<
        'static,
        Result<
            (
                Option<MPath>,
                Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>,
            ),
            Error,
        >,
    >
    where
        I: IntoIterator<Item = P>,
        PathOrPrefix: From<P>,
    {
        let selector = select_path_tree(paths_or_prefixes);

        // Schedule a maximum of 256 concurrently unfolding directories.
        let schedule_max = nonzero!(256usize);

        // Allow queueing of up to 2,560 items, which would be 10 items per
        // directory at the maximum concurrency level.  Experiments show this
        // is a good balance of queueing items while not spending too long
        // determining what can be scheduled.
        let queue_max = nonzero!(2560usize);

        let after = after.into();

        let init = Some((
            queue_max.get(),
            (self.clone(), selector, None, false, after),
        ));
        (async_stream::stream! {
            borrowed!(ctx, store);
            let s = bounded_traversal::bounded_traversal_ordered_stream(
                schedule_max,
                queue_max,
                init,
                move |(manifest_id, selector, path, recursive, after)| {
                    let PathTree {
                        subentries,
                        value: select,
                    } = selector;

                    async move {
                        let manifest = manifest_id.load(ctx, store).await?;

                        let mut output = Vec::new();

                        if recursive || select.is_recursive() {
                            if after.include_self() {
                                output.push(OrderedTraversal::Output((
                                    path.clone(),
                                    Entry::Tree(manifest_id),
                                )));
                            }
                            let mut stream = manifest.list_weighted(ctx, store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                if after.skip(&name) {
                                    continue;
                                }
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                match entry {
                                    Entry::Leaf(leaf) => {
                                        if after.include_file(&name) {
                                            output.push(OrderedTraversal::Output((
                                                path.clone(),
                                                Entry::Leaf(leaf),
                                            )));
                                        }
                                    }
                                    Entry::Tree((weight, manifest_id)) => {
                                        output.push(OrderedTraversal::Recurse(
                                            weight,
                                            (
                                                manifest_id,
                                                Default::default(),
                                                path,
                                                true,
                                                after.enter_dir(&name),
                                            ),
                                        ));
                                    }
                                }
                            }
                        } else {
                            if after.include_self() && select.is_selected() {
                                output.push(OrderedTraversal::Output((
                                    path.clone(),
                                    Entry::Tree(manifest_id),
                                )));
                            }
                            for (name, selector) in subentries {
                                if after.skip(&name) {
                                    continue;
                                }
                                if let Some(entry) = manifest.lookup_weighted(ctx, store, &name).await? {
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                    match entry {
                                        Entry::Leaf(leaf) => {
                                            if after.include_file(&name)
                                                && selector.value.is_selected()
                                            {
                                                output.push(OrderedTraversal::Output((
                                                    path.clone(),
                                                    Entry::Leaf(leaf),
                                                )));
                                            }
                                        }
                                        Entry::Tree((weight, manifest_id)) => {
                                            output.push(OrderedTraversal::Recurse(
                                                weight,
                                                (
                                                    manifest_id,
                                                    selector,
                                                    path,
                                                    false,
                                                    after.enter_dir(&name),
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        Ok::<_, Error>(output)
                    }
                    .boxed()
                },
            );

            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        })
        .boxed()
    }

    /// Returns ordered differences between two manifests.
    ///
    /// `self` is considered the "old" manifest (so entries missing there are "Added")
    /// `other` is considered the "new" manifest (so entries missing there are "Removed")
    fn diff_ordered(
        &self,
        ctx: CoreContext,
        store: Store,
        other: Self,
        after: Option<Option<MPath>>,
    ) -> BoxStream<
        'static,
        Result<
            Diff<Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>>,
            Error,
        >,
    > {
        self.filtered_diff_ordered(ctx, store.clone(), other, store, after, Some, |_| true)
    }

    /// Do a diff, but with knobs to filter_map output and prune some subtrees.
    /// `output_filter` let's us configure what will be returned from filtered_diff. it accepts
    /// every diff entry and returns Option<Out>, so it acts similar to filter_map() function
    /// recurse_pruner is a function that allows us to skip iterating over some subtrees
    fn filtered_diff_ordered<FilterMap, Out, RecursePruner>(
        &self,
        ctx: CoreContext,
        store: Store,
        other: Self,
        other_store: Store,
        after: Option<Option<MPath>>,
        output_filter: FilterMap,
        recurse_pruner: RecursePruner,
    ) -> BoxStream<'static, Result<Out, Error>>
    where
        FilterMap: Fn(
                Diff<
                    Entry<Self, <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId>,
                >,
            ) -> Option<Out>
            + Send
            + Sync
            + 'static,
        RecursePruner: Fn(&Diff<Self>) -> bool + Send + Sync + 'static,
        Out: Send + Unpin + 'static,
    {
        if self == &other {
            return stream::empty().boxed();
        }

        // Schedule a maximum of 256 concurrently unfolding directories.
        let schedule_max = nonzero!(256usize);

        // Allow queueing of up to 2,560 items, which would be 10 items per
        // directory at the maximum concurrency level.  Experiments show this
        // is a good balance of queueing items while not spending too long
        // determining what can be scheduled.
        let queue_max = nonzero!(2560usize);

        let after = match after {
            None => {
                // If `after` is `None`, then we include everything.
                After::All
            }
            Some(mpath_opt) => {
                // If `after` is `Some(None)`, then we include everything
                // after the root (i.e. not the root itself).
                After::new(mpath_opt.as_ref())
            }
        };

        let init = Some((
            queue_max.get(),
            (Diff::Changed(None, self.clone(), other), after),
        ));

        (async_stream::stream! {
            borrowed!(ctx, store, other_store, output_filter, recurse_pruner);

            let s = bounded_traversal::bounded_traversal_ordered_stream(
                schedule_max,
                queue_max,
                init,
                move |(input, after)| {
                    async move {
                        let mut output = Vec::new();

                        let push_output = |output: &mut Vec<_>, out| {
                            if let Some(out) = output_filter(out) {
                                output.push(OrderedTraversal::Output(out));
                            }
                        };

                        let push_recurse = |output: &mut Vec<_>, weight, recurse, after| {
                            if recurse_pruner(&recurse) {
                                output.push(OrderedTraversal::Recurse(weight, (recurse, after)));
                            }
                        };

                        match input {
                            Diff::Changed(path, left, right) => {
                                let (left_mf, right_mf) = future::try_join(
                                    left.load(ctx, store),
                                    right.load(ctx, other_store),
                                )
                                .await?;

                                if after.include_self() {
                                    push_output(
                                        &mut output,
                                        Diff::Changed(
                                            path.clone(),
                                            Entry::Tree(left),
                                            Entry::Tree(right),
                                        ),
                                    );
                                }

                                let iter = EntryDiffIterator::new(
                                    left_mf.list_weighted(ctx, store).await?.try_collect::<Vec<_>>().await?.into_iter(),
                                    right_mf.list_weighted(ctx, other_store).await?.try_collect::<Vec<_>>().await?.into_iter(),
                                );
                                for (name, left, right) in iter {
                                    if after.skip(&name) || left == right {
                                        continue;
                                    }
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                    match (left, right) {
                                        (Some(Entry::Leaf(left)), Some(Entry::Leaf(right))) => {
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Changed(
                                                        path,
                                                        Entry::Leaf(left),
                                                        Entry::Leaf(right),
                                                    ),
                                                );
                                            }
                                        }
                                        (
                                            Some(Entry::Leaf(left)),
                                            Some(Entry::Tree((weight, tree))),
                                        ) => {
                                            // Removed file comes before all
                                            // files in the dir it is replaced
                                            // by.
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Removed(path.clone(), Entry::Leaf(left)),
                                                );
                                            }
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Added(path, tree),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        (Some(Entry::Leaf(left)), None) => {
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Removed(path, Entry::Leaf(left)),
                                                );
                                            }
                                        }
                                        (
                                            Some(Entry::Tree((weight, tree))),
                                            Some(Entry::Leaf(right)),
                                        ) => {
                                            // Added file comes before all
                                            // files in the dir it replaces
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Added(path.clone(), Entry::Leaf(right)),
                                                );
                                            }
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Removed(path, tree),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        (
                                            Some(Entry::Tree((left_weight, left))),
                                            Some(Entry::Tree((right_weight, right))),
                                        ) => {
                                            // Approximate recursion weight
                                            // using `max`.  The theoretical
                                            // max is actually the sum of the
                                            // weights, but that is likely to
                                            // be overkill most of the time.
                                            let weight = left_weight.max(right_weight);
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Changed(path, left, right),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        (Some(Entry::Tree((weight, tree))), None) => {
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Removed(path, tree),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        (None, Some(Entry::Leaf(right))) => {
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Added(path.clone(), Entry::Leaf(right)),
                                                );
                                            }
                                        }
                                        (None, Some(Entry::Tree((weight, tree)))) => {
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Added(path, tree),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        (None, None) => {}
                                    }
                                }
                            }
                            Diff::Added(path, tree) => {
                                if after.include_self() {
                                    push_output(
                                        &mut output,
                                        Diff::Added(path.clone(), Entry::Tree(tree.clone())),
                                    );
                                }
                                let manifest = tree.load(ctx, other_store).await?;
                            let mut stream = manifest.list_weighted(ctx, store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                    if after.skip(&name) {
                                        continue;
                                    }
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                    match entry {
                                        Entry::Tree((weight, tree)) => {
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Added(path, tree),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        Entry::Leaf(leaf) => {
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Added(path, Entry::Leaf(leaf)),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Diff::Removed(path, tree) => {
                                if after.include_self() {
                                    push_output(
                                        &mut output,
                                        Diff::Removed(path.clone(), Entry::Tree(tree.clone())),
                                    );
                                }
                                let manifest = tree.load(ctx, store).await?;
                            let mut stream = manifest.list_weighted(ctx, store).await?;
                            while let Some((name, entry)) = stream.try_next().await? {
                                    if after.skip(&name) {
                                        continue;
                                    }
                                    let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                    match entry {
                                        Entry::Tree((weight, tree)) => {
                                            push_recurse(
                                                &mut output,
                                                weight,
                                                Diff::Removed(path, tree),
                                                after.enter_dir(&name),
                                            );
                                        }
                                        Entry::Leaf(leaf) => {
                                            if after.include_file(&name) {
                                                push_output(
                                                    &mut output,
                                                    Diff::Removed(path, Entry::Leaf(leaf)),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(output)
                    }
                    .boxed()
                },
            );

            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        })
        .boxed()
    }
}

struct EntryDiffIterator<I>
where
    I: Iterator,
{
    left: Peekable<I>,
    right: Peekable<I>,
}

impl<I> EntryDiffIterator<I>
where
    I: Iterator,
{
    fn new(left: I, right: I) -> Self {
        Self {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<I, Name, Value> Iterator for EntryDiffIterator<I>
where
    I: Iterator<Item = (Name, Value)>,
    Name: Ord,
{
    type Item = (Name, Option<Value>, Option<Value>);

    fn next(&mut self) -> Option<Self::Item> {
        match (self.left.peek(), self.right.peek()) {
            (Some((left_name, _)), Some((right_name, _))) => match left_name.cmp(right_name) {
                Ordering::Less => {
                    let (name, left) = self.left.next().unwrap();
                    Some((name, Some(left), None))
                }
                Ordering::Equal => {
                    let (name, left) = self.left.next().unwrap();
                    let (_, right) = self.right.next().unwrap();
                    Some((name, Some(left), Some(right)))
                }
                Ordering::Greater => {
                    let (name, right) = self.right.next().unwrap();
                    Some((name, None, Some(right)))
                }
            },
            (Some(_), None) => {
                let (name, left) = self.left.next().unwrap();
                Some((name, Some(left), None))
            }
            (None, Some(_)) => {
                let (name, right) = self.right.next().unwrap();
                Some((name, None, Some(right)))
            }
            (None, None) => None,
        }
    }
}

impl<TreeId, Store> ManifestOrderedOps<Store> for TreeId
where
    Store: Sync + Send + Clone + 'static,
    Self: StoreLoadable<Store> + Clone + Send + Sync + Eq + Unpin + 'static,
    <Self as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = Self> + OrderedManifest<Store> + Send + Sync,
    <<Self as StoreLoadable<Store>>::Value as Manifest<Store>>::LeafId: Send + Clone + Eq + Unpin,
{
}
