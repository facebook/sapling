// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{Entry, Manifest, PathTree};
use blobstore::{Blobstore, Loadable};
use context::CoreContext;
use failure::Error;
use futures::{stream, Future, Stream};
use futures_ext::{bounded_traversal::bounded_traversal_stream, BoxStream, FutureExt, StreamExt};
use mononoke_types::MPath;
use std::iter::FromIterator;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Diff<Entry> {
    Added(Option<MPath>, Entry),
    Removed(Option<MPath>, Entry),
    Changed(Option<MPath>, Entry, Entry),
}

pub trait ManifestOps
where
    Self: Loadable + Copy + Send + Eq,
    <Self as Loadable>::Value: Manifest<TreeId = Self> + Send,
    <<Self as Loadable>::Value as Manifest>::LeafId: Copy + Send + Eq,
{
    fn find_entries(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
        paths: impl IntoIterator<Item = MPath>,
    ) -> BoxStream<
        (
            MPath,
            Entry<Self, <<Self as Loadable>::Value as Manifest>::LeafId>,
        ),
        Error,
    > {
        let selector = PathTree::from_iter(paths.into_iter().map(|path| (path, true)));
        bounded_traversal_stream(
            256,
            (selector, None, self.clone()),
            move |(PathTree { subentries, .. }, path, manifest_id)| {
                manifest_id
                    .load(ctx.clone(), &blobstore)
                    .map(move |manifest| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();
                        for (name, subentry) in subentries {
                            if let Some(entry) = manifest.lookup(&name) {
                                let path = MPath::join_opt_element(path.as_ref(), &name);
                                if subentry.value {
                                    output.push((path.clone(), entry.clone()));
                                }
                                if let Entry::Tree(manifest_id) = entry {
                                    recurse.push((subentry, Some(path), manifest_id));
                                }
                            }
                        }
                        (output, recurse)
                    })
            },
        )
        .map(|entries| stream::iter_ok(entries))
        .flatten()
        .boxify()
    }

    fn diff(
        &self,
        ctx: CoreContext,
        blobstore: impl Blobstore + Clone,
        other: Self,
    ) -> BoxStream<Diff<Entry<Self, <<Self as Loadable>::Value as Manifest>::LeafId>>, Error> {
        if self == &other {
            return stream::empty().boxify();
        }

        bounded_traversal_stream(
            256,
            Diff::Changed(None, self.clone(), other),
            move |input| match input {
                Diff::Changed(path, left, right) => left
                    .load(ctx.clone(), &blobstore)
                    .join(right.load(ctx.clone(), &blobstore))
                    .map(move |(left_mf, right_mf)| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();

                        for (name, left) in left_mf.list() {
                            let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                            if let Some(right) = right_mf.lookup(&name) {
                                if left != right {
                                    match (left, right) {
                                        (Entry::Leaf(_), Entry::Leaf(_)) => {
                                            output.push(Diff::Changed(path, left, right))
                                        }
                                        (Entry::Tree(tree), Entry::Leaf(_)) => {
                                            output.push(Diff::Added(path.clone(), right));
                                            recurse.push(Diff::Removed(path, tree));
                                        }
                                        (Entry::Leaf(_), Entry::Tree(tree)) => {
                                            output.push(Diff::Removed(path.clone(), left));
                                            recurse.push(Diff::Added(path, tree));
                                        }
                                        (Entry::Tree(left), Entry::Tree(right)) => {
                                            recurse.push(Diff::Changed(path, left, right))
                                        }
                                    }
                                }
                            } else {
                                match left {
                                    Entry::Tree(tree) => recurse.push(Diff::Removed(path, tree)),
                                    _ => output.push(Diff::Removed(path, left)),
                                }
                            }
                        }
                        for (name, right) in right_mf.list() {
                            if left_mf.lookup(&name).is_none() {
                                let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                                match right {
                                    Entry::Tree(tree) => recurse.push(Diff::Added(path, tree)),
                                    _ => output.push(Diff::Added(path, right)),
                                }
                            }
                        }
                        output.push(Diff::Changed(path, Entry::Tree(left), Entry::Tree(right)));

                        (output, recurse)
                    })
                    .left_future(),
                Diff::Added(path, tree) => {
                    tree.load(ctx.clone(), &blobstore).map(move |manifest| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();
                        for (name, entry) in manifest.list() {
                            let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                            match entry {
                                Entry::Tree(tree) => recurse.push(Diff::Added(path, tree)),
                                _ => output.push(Diff::Added(path, entry)),
                            }
                        }
                        output.push(Diff::Added(path, Entry::Tree(tree)));
                        (output, recurse)
                    })
                }
                .left_future()
                .right_future(),
                Diff::Removed(path, tree) => {
                    tree.load(ctx.clone(), &blobstore).map(move |manifest| {
                        let mut output = Vec::new();
                        let mut recurse = Vec::new();
                        for (name, entry) in manifest.list() {
                            let path = Some(MPath::join_opt_element(path.as_ref(), &name));
                            match entry {
                                Entry::Tree(tree) => recurse.push(Diff::Removed(path, tree)),
                                _ => output.push(Diff::Removed(path, entry)),
                            }
                        }
                        output.push(Diff::Removed(path, Entry::Tree(tree)));
                        (output, recurse)
                    })
                }
                .right_future()
                .right_future(),
            },
        )
        .map(|output| stream::iter_ok(output))
        .flatten()
        .boxify()
    }
}

impl<TreeId> ManifestOps for TreeId
where
    Self: Loadable + Copy + Send + Eq,
    <Self as Loadable>::Value: Manifest<TreeId = Self> + Send,
    <<Self as Loadable>::Value as Manifest>::LeafId: Send + Copy + Eq,
{
}
