// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use context::CoreContext;
use futures::future::{self, Future};
use futures::stream::{empty, once, Stream};
use futures::IntoFuture;
use futures_ext::{select_all, BoxFuture, BoxStream, FutureExt, StreamExt};
use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::FromIterator;

use super::manifest::{Content, EmptyManifest, Type};
use super::{Entry, MPath, MPathElement, Manifest};
use crate::errors::*;

// Note that:
// * this isn't "left" and "right" because an explicit direction makes the API clearer
// * this isn't "new" and "old" because one could ask for either a diff from a child manifest to
//   its parent, or the other way round
#[derive(Debug)]
pub enum EntryStatus {
    Added(Box<dyn Entry + Sync>),
    Deleted(Box<dyn Entry + Sync>),
    // Entries will always either be File or Tree. However, it's possible for one of the entries
    // to be Regular and the other to be Symlink, etc.
    Modified {
        to_entry: Box<dyn Entry + Sync>,
        from_entry: Box<dyn Entry + Sync>,
    },
}

impl EntryStatus {
    /// Whether this status represents a tree.
    ///
    /// If a tree is replaced by a file or vice versa, it will always be represented as an `Added`
    /// and a `Deleted`, not a single `Modified`.
    pub fn is_tree(&self) -> bool {
        match self {
            EntryStatus::Added(entry) => entry.get_type().is_tree(),
            EntryStatus::Deleted(entry) => entry.get_type().is_tree(),
            EntryStatus::Modified {
                to_entry,
                from_entry,
            } => {
                debug_assert_eq!(
                    to_entry.get_type().is_tree(),
                    from_entry.get_type().is_tree()
                );
                to_entry.get_type().is_tree()
            }
        }
    }

    /// Whether this status represents a file.
    ///
    /// If a tree is replaced by a file or vice versa, it will always be represented as an `Added`
    /// and a `Deleted`, not a single `Modified`.
    #[inline]
    pub fn is_file(&self) -> bool {
        !self.is_tree()
    }
}

#[derive(Debug)]
pub struct ChangedEntry {
    pub dirname: Option<MPath>,
    pub status: EntryStatus,
}

impl ChangedEntry {
    pub fn new_added(dirname: Option<MPath>, entry: Box<dyn Entry + Sync>) -> Self {
        ChangedEntry {
            dirname,
            status: EntryStatus::Added(entry),
        }
    }

    pub fn new_deleted(dirname: Option<MPath>, entry: Box<dyn Entry + Sync>) -> Self {
        ChangedEntry {
            dirname,
            status: EntryStatus::Deleted(entry),
        }
    }

    pub fn new_modified(
        dirname: Option<MPath>,
        to_entry: Box<dyn Entry + Sync>,
        from_entry: Box<dyn Entry + Sync>,
    ) -> Self {
        ChangedEntry {
            dirname,
            status: EntryStatus::Modified {
                to_entry,
                from_entry,
            },
        }
    }

    pub fn get_full_path(&self) -> Option<MPath> {
        match &self.status {
            EntryStatus::Added(entry) => {
                let dirname = self.dirname.clone();
                let entry_path = entry.get_name().cloned();
                MPath::join_element_opt(dirname.as_ref(), entry_path.as_ref())
            }
            EntryStatus::Deleted(entry) => {
                let dirname = self.dirname.clone();
                let entry_path = entry.get_name().cloned();
                MPath::join_element_opt(dirname.as_ref(), entry_path.as_ref())
            }
            EntryStatus::Modified {
                to_entry,
                from_entry,
            } => {
                debug_assert!(to_entry.get_type().is_tree() == from_entry.get_type().is_tree());

                let dirname = self.dirname.clone();
                let entry_path = to_entry.get_name().cloned();
                MPath::join_element_opt(dirname.as_ref(), entry_path.as_ref())
            }
        }
    }
}

impl fmt::Display for ChangedEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let full_path = self.get_full_path();
        let path_display = MPath::display_opt(full_path.as_ref());

        match &self.status {
            EntryStatus::Added(entry) => write!(
                f,
                "[added] path: {}, hash: {}, type: {}",
                path_display,
                entry.get_hash(),
                entry.get_type(),
            ),
            EntryStatus::Deleted(entry) => write!(
                f,
                "[deleted] path: {}, hash: {}, type: {}",
                path_display,
                entry.get_hash(),
                entry.get_type(),
            ),
            EntryStatus::Modified {
                to_entry,
                from_entry,
            } => write!(
                f,
                "[modified] path: {}, to {{hash: {}, type: {}}}, from {{hash: {}, type: {}}}",
                path_display,
                to_entry.get_hash(),
                to_entry.get_type(),
                from_entry.get_hash(),
                from_entry.get_type(),
            ),
        }
    }
}

struct NewEntry {
    dirname: Option<MPath>,
    entry: Box<dyn Entry + Sync>,
}

impl NewEntry {
    fn from_changed_entry(ce: ChangedEntry) -> Option<Self> {
        let dirname = ce.dirname;
        match ce.status {
            EntryStatus::Deleted(_) => None,
            EntryStatus::Added(entry)
            | EntryStatus::Modified {
                to_entry: entry, ..
            } => Some(Self { dirname, entry }),
        }
    }

    fn into_tuple(self) -> (Option<MPath>, Box<dyn Entry + Sync>) {
        (self.dirname, self.entry)
    }
}

impl PartialEq for NewEntry {
    fn eq(&self, other: &Self) -> bool {
        self.dirname == other.dirname
    }
}
impl Eq for NewEntry {}

impl Hash for NewEntry {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.dirname.hash(state);
    }
}

/// For a given Manifests and list of parents this function recursively compares their content and
/// returns a intersection of entries that the given Manifest had added (both newly added and
/// replacement for modified entries) compared to it's parents
///
/// TODO(luk, T26981580) This implementation is not efficient, because in order to find the
///                      intersection of parents it first produces the full difference of root vs
///                      each parent, then puts /// one parent in a HashSet and performs the
///                      intersection.
///                      A better approach would be to traverse the manifest tree of root and both
///                      parents simultaniously and produce the intersection result while
///                      traversing
pub fn new_entry_intersection_stream<M, P1M, P2M>(
    ctx: CoreContext,
    root: &M,
    p1: Option<&P1M>,
    p2: Option<&P2M>,
) -> BoxStream<(Option<MPath>, Box<dyn Entry + Sync>), Error>
where
    M: Manifest,
    P1M: Manifest,
    P2M: Manifest,
{
    if p1.is_none() || p2.is_none() {
        let ces = if let Some(p1) = p1 {
            changed_entry_stream(ctx, root, p1, None)
        } else if let Some(p2) = p2 {
            changed_entry_stream(ctx, root, p2, None)
        } else {
            changed_entry_stream(ctx, root, &EmptyManifest {}, None)
        };

        ces.filter_map(NewEntry::from_changed_entry)
            .map(NewEntry::into_tuple)
            .boxify()
    } else {
        let p1 = changed_entry_stream(ctx.clone(), root, p1.unwrap(), None)
            .filter_map(NewEntry::from_changed_entry);
        let p2 = changed_entry_stream(ctx, root, p2.unwrap(), None)
            .filter_map(NewEntry::from_changed_entry);

        p2.collect()
            .map(move |p2| {
                let p2: HashSet<_> = HashSet::from_iter(p2.into_iter());

                p1.filter_map(move |ne| if p2.contains(&ne) { Some(ne) } else { None })
            })
            .flatten_stream()
            .map(NewEntry::into_tuple)
            .boxify()
    }
}

pub trait Pruner {
    fn keep(&mut self, entry: &ChangedEntry) -> bool;
}

#[derive(Clone)]
pub struct NoopPruner;

impl Pruner for NoopPruner {
    fn keep(&mut self, _: &ChangedEntry) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct FilePruner;

impl Pruner for FilePruner {
    fn keep(&mut self, entry: &ChangedEntry) -> bool {
        entry.status.is_tree()
    }
}

#[derive(Clone)]
pub struct DeletedPruner;

impl Pruner for DeletedPruner {
    fn keep(&mut self, entry: &ChangedEntry) -> bool {
        match entry.status {
            EntryStatus::Deleted(..) => false,
            _ => true,
        }
    }
}

#[derive(Clone)]
pub struct CombinatorPruner<A, B> {
    a: A,
    b: B,
}

impl<A: Pruner, B: Pruner> CombinatorPruner<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<A: Pruner, B: Pruner> Pruner for CombinatorPruner<A, B> {
    fn keep(&mut self, entry: &ChangedEntry) -> bool {
        self.a.keep(entry) && self.b.keep(entry)
    }
}

/// Given two manifests, returns a difference between them. Difference is a stream of
/// ChangedEntry, each showing whether a file/directory was added, deleted or modified.
/// Note: Modified entry contains only entries of the same type i.e. if a file was replaced
/// with a directory of the same name, then returned stream will contain Deleted file entry,
/// and Added directory entry. The same *does not* apply for changes between the various
/// file types (Regular, Executable and Symlink): those will only be one Modified entry.
pub fn changed_entry_stream<TM, FM>(
    ctx: CoreContext,
    to: &TM,
    from: &FM,
    path: Option<MPath>,
) -> BoxStream<ChangedEntry, Error>
where
    TM: Manifest,
    FM: Manifest,
{
    changed_entry_stream_with_pruner(ctx, to, from, path, NoopPruner, None).boxify()
}

pub fn changed_file_stream<TM, FM>(
    ctx: CoreContext,
    to: &TM,
    from: &FM,
    path: Option<MPath>,
) -> BoxStream<ChangedEntry, Error>
where
    TM: Manifest,
    FM: Manifest,
{
    changed_entry_stream_with_pruner(ctx, to, from, path, NoopPruner, None)
        .filter(|changed_entry| !changed_entry.status.is_tree())
        .boxify()
}

pub fn changed_entry_stream_with_pruner<TM, FM>(
    ctx: CoreContext,
    to: &TM,
    from: &FM,
    path: Option<MPath>,
    pruner: impl Pruner + Send + Clone + 'static,
    max_depth: Option<usize>,
) -> impl Stream<Item = ChangedEntry, Error = Error>
where
    TM: Manifest,
    FM: Manifest,
{
    if max_depth == Some(0) {
        return empty().boxify();
    }

    diff_manifests(path, to, from)
        .map(move |diff| {
            select_all(
                diff.into_iter()
                    .filter({
                        let mut pruner = pruner.clone();
                        move |entry| pruner.keep(entry)
                    })
                    .map(|entry| {
                        recursive_changed_entry_stream(
                            ctx.clone(),
                            entry,
                            1,
                            pruner.clone(),
                            max_depth,
                        )
                    }),
            )
        })
        .flatten_stream()
        .boxify()
}

/// Given a ChangedEntry, return a stream that consists of this entry, and all subentries
/// that differ. If input isn't a tree, then a stream with a single entry is returned, otherwise
/// subtrees are recursively compared.
fn recursive_changed_entry_stream(
    ctx: CoreContext,
    changed_entry: ChangedEntry,
    depth: usize,
    pruner: impl Pruner + Send + Clone + 'static,
    max_depth: Option<usize>,
) -> BoxStream<ChangedEntry, Error> {
    if !changed_entry.status.is_tree() || (max_depth.is_some() && max_depth <= Some(depth)) {
        return once(Ok(changed_entry)).boxify();
    }

    let (to_mf, from_mf, path) = match &changed_entry.status {
        EntryStatus::Added(entry) => {
            let empty_mf: Box<dyn Manifest> = Box::new(EmptyManifest {});
            let to_mf = entry
                .get_content(ctx.clone())
                .map(get_tree_content)
                .boxify();
            let from_mf = Ok(empty_mf).into_future().boxify();

            let dirname = changed_entry.dirname.clone();
            let entry_path = entry.get_name().cloned();
            let path = MPath::join_element_opt(dirname.as_ref(), entry_path.as_ref());

            (to_mf, from_mf, path)
        }
        EntryStatus::Deleted(entry) => {
            let empty_mf: Box<dyn Manifest> = Box::new(EmptyManifest {});
            let to_mf = Ok(empty_mf).into_future().boxify();
            let from_mf = entry
                .get_content(ctx.clone())
                .map(get_tree_content)
                .boxify();

            let dirname = changed_entry.dirname.clone();
            let entry_path = entry.get_name().cloned();
            let path = MPath::join_element_opt(dirname.as_ref(), entry_path.as_ref());

            (to_mf, from_mf, path)
        }
        EntryStatus::Modified {
            to_entry,
            from_entry,
        } => {
            debug_assert!(to_entry.get_type().is_tree() == from_entry.get_type().is_tree());
            debug_assert!(to_entry.get_type().is_tree());

            let to_mf = to_entry
                .get_content(ctx.clone())
                .map(get_tree_content)
                .boxify();
            let from_mf = from_entry
                .get_content(ctx.clone())
                .map(get_tree_content)
                .boxify();

            let dirname = changed_entry.dirname.clone();
            let entry_path = to_entry.get_name().cloned();
            let path = MPath::join_element_opt(dirname.as_ref(), entry_path.as_ref());

            (to_mf, from_mf, path)
        }
    };

    let substream = to_mf
        .join(from_mf)
        .map(move |(to_mf, from_mf)| {
            diff_manifests(path, &to_mf, &from_mf)
                .map(move |diff| {
                    select_all(
                        diff.into_iter()
                            .filter({
                                let mut pruner = pruner.clone();
                                move |entry| pruner.keep(entry)
                            })
                            .map(|entry| {
                                recursive_changed_entry_stream(
                                    ctx.clone(),
                                    entry,
                                    depth + 1,
                                    pruner.clone(),
                                    max_depth,
                                )
                            }),
                    )
                })
                .flatten_stream()
        })
        .flatten_stream();

    once(Ok(changed_entry)).chain(substream).boxify()
}

/// Given an entry and path from the root of the repo to this entry, returns all subentries with
/// their path from the root of the repo.
/// For a non-tree entry returns a stream with a single (entry, path) pair.
pub fn recursive_entry_stream(
    ctx: CoreContext,
    rootpath: Option<MPath>,
    entry: Box<dyn Entry + Sync>,
) -> BoxStream<(Option<MPath>, Box<dyn Entry + Sync>), Error> {
    let subentries =
        match entry.get_type() {
            Type::File(_) => empty().boxify(),
            Type::Tree => {
                let entry_basename = entry.get_name();
                let path = MPath::join_opt(rootpath.as_ref(), entry_basename);

                entry
                    .get_content(ctx.clone())
                    .map(|content| {
                        select_all(get_tree_content(content).list().map(move |entry| {
                            recursive_entry_stream(ctx.clone(), path.clone(), entry)
                        }))
                    })
                    .flatten_stream()
                    .boxify()
            }
        };

    once(Ok((rootpath, entry))).chain(subentries).boxify()
}

/// Difference between manifests, non-recursive.
/// It fetches manifest content, sorts it and compares.
fn diff_manifests<TM, FM>(
    path: Option<MPath>,
    to: &TM,
    from: &FM,
) -> BoxFuture<Vec<ChangedEntry>, Error>
where
    TM: Manifest,
    FM: Manifest,
{
    let to_vec: Vec<_> = to.list().collect();
    let from_vec: Vec<_> = from.list().collect();

    // XXX this does not need to be a future at all
    future::ok(diff_sorted_vecs(path, to_vec, from_vec)).boxify()
}

/// Compares vectors of entries and returns the difference
// TODO(stash): T25644857 this method is made public to make it possible to test it.
// Otherwise we need create dependency to mercurial_types_mocks, which depends on mercurial_types.
// This causing compilation failure.
// We need to find a workaround for an issue.
pub fn diff_sorted_vecs(
    path: Option<MPath>,
    to: Vec<Box<dyn Entry + Sync>>,
    from: Vec<Box<dyn Entry + Sync>>,
) -> Vec<ChangedEntry> {
    let mut to = VecDeque::from(to);
    let mut from = VecDeque::from(from);

    let mut res = vec![];
    loop {
        match (to.pop_front(), from.pop_front()) {
            (Some(to_entry), Some(from_entry)) => {
                let to_path: Option<MPathElement> = to_entry.get_name().cloned();
                let from_path: Option<MPathElement> = from_entry.get_name().cloned();

                // note that for Option types, None is less than any Some
                if to_path < from_path {
                    res.push(ChangedEntry::new_added(path.clone(), to_entry));
                    from.push_front(from_entry);
                } else if to_path > from_path {
                    res.push(ChangedEntry::new_deleted(path.clone(), from_entry));
                    to.push_front(to_entry);
                } else {
                    if to_entry.get_type().is_tree() == from_entry.get_type().is_tree() {
                        if to_entry.get_hash() != from_entry.get_hash()
                            || to_entry.get_type() != from_entry.get_type()
                        {
                            res.push(ChangedEntry::new_modified(
                                path.clone(),
                                to_entry,
                                from_entry,
                            ));
                        }
                    } else {
                        res.push(ChangedEntry::new_added(path.clone(), to_entry));
                        res.push(ChangedEntry::new_deleted(path.clone(), from_entry));
                    }
                }
            }

            (Some(to_entry), None) => {
                res.push(ChangedEntry::new_added(path.clone(), to_entry));
            }

            (None, Some(from_entry)) => {
                res.push(ChangedEntry::new_deleted(path.clone(), from_entry));
            }
            (None, None) => {
                break;
            }
        }
    }

    res
}

fn get_tree_content(content: Content) -> Box<dyn Manifest> {
    match content {
        Content::Tree(manifest) => manifest,
        _ => panic!("Tree entry was expected"),
    }
}
