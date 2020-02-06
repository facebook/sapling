/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::iter::FromIterator;

use anyhow::Error;
use futures::future::Future;
use futures::stream::{empty, iter_ok, once, Stream};
use futures_ext::{BoxStream, StreamExt};

use mercurial_types::{MPath, MPathElement, Type};

use super::revlog::EntryContent;
use super::{RevlogEntry, RevlogManifest};

// Note that:
// * this isn't "left" and "right" because an explicit direction makes the API clearer
// * this isn't "new" and "old" because one could ask for either a diff from a child manifest to
//   its parent, or the other way round
pub enum EntryStatus {
    Added(RevlogEntry),
    Deleted(RevlogEntry),
    // Entries will always either be File or Tree. However, it's possible for one of the entries
    // to be Regular and the other to be Symlink, etc.
    Modified {
        to_entry: RevlogEntry,
        from_entry: RevlogEntry,
    },
}

pub struct ChangedEntry {
    pub path: Option<MPath>,
    pub status: EntryStatus,
}

impl ChangedEntry {
    pub fn new_added(path: Option<MPath>, entry: RevlogEntry) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Added(entry),
        }
    }

    pub fn new_deleted(path: Option<MPath>, entry: RevlogEntry) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Deleted(entry),
        }
    }

    pub fn new_modified(
        path: Option<MPath>,
        to_entry: RevlogEntry,
        from_entry: RevlogEntry,
    ) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Modified {
                to_entry,
                from_entry,
            },
        }
    }
}

struct NewEntry {
    path: Option<MPath>,
    entry: RevlogEntry,
}

impl NewEntry {
    fn from_changed_entry(ce: ChangedEntry) -> Option<Self> {
        let path = ce.path;
        match ce.status {
            EntryStatus::Deleted(_) => None,
            EntryStatus::Added(entry)
            | EntryStatus::Modified {
                to_entry: entry, ..
            } => Some(Self { path, entry }),
        }
    }

    fn into_tuple(self) -> (Option<MPath>, RevlogEntry) {
        (self.path, self.entry)
    }
}

impl PartialEq for NewEntry {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
impl Eq for NewEntry {}

impl Hash for NewEntry {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.path.hash(state);
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
pub fn new_entry_intersection_stream(
    root: &RevlogManifest,
    p1: Option<&RevlogManifest>,
    p2: Option<&RevlogManifest>,
) -> BoxStream<(Option<MPath>, RevlogEntry), Error> {
    if p1.is_none() || p2.is_none() {
        let ces = if let Some(p1) = p1 {
            changed_entry_stream(root, p1, None)
        } else if let Some(p2) = p2 {
            changed_entry_stream(root, p2, None)
        } else {
            changed_entry_stream(root, &RevlogManifest::empty(), None)
        };

        ces.filter_map(NewEntry::from_changed_entry)
            .map(NewEntry::into_tuple)
            .boxify()
    } else {
        let p1 =
            changed_entry_stream(root, p1.unwrap(), None).filter_map(NewEntry::from_changed_entry);
        let p2 =
            changed_entry_stream(root, p2.unwrap(), None).filter_map(NewEntry::from_changed_entry);

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

/// Given two manifests, returns a difference between them. Difference is a stream of
/// ChangedEntry, each showing whether a file/directory was added, deleted or modified.
/// Note: Modified entry contains only entries of the same type i.e. if a file was replaced
/// with a directory of the same name, then returned stream will contain Deleted file entry,
/// and Added directory entry. The same *does not* apply for changes between the various
/// file types (Regular, Executable and Symlink): those will only be one Modified entry.
pub fn changed_entry_stream(
    to: &RevlogManifest,
    from: &RevlogManifest,
    path: Option<MPath>,
) -> BoxStream<ChangedEntry, Error> {
    diff_manifests(path, to, from)
        .map(recursive_changed_entry_stream)
        .flatten()
        .boxify()
}

/// Given a ChangedEntry, return a stream that consists of this entry, and all subentries
/// that differ. If input isn't a tree, then a stream with a single entry is returned, otherwise
/// subtrees are recursively compared.
fn recursive_changed_entry_stream(changed_entry: ChangedEntry) -> BoxStream<ChangedEntry, Error> {
    match changed_entry.status {
        EntryStatus::Added(entry) => recursive_entry_stream(changed_entry.path, entry)
            .map(|(path, entry)| ChangedEntry::new_added(path, entry))
            .boxify(),
        EntryStatus::Deleted(entry) => recursive_entry_stream(changed_entry.path, entry)
            .map(|(path, entry)| ChangedEntry::new_deleted(path, entry))
            .boxify(),
        EntryStatus::Modified {
            to_entry,
            from_entry,
        } => {
            debug_assert!(to_entry.get_type().is_tree() == from_entry.get_type().is_tree());

            let substream = if to_entry.get_type().is_tree() {
                let contents = to_entry.get_content().join(from_entry.get_content());
                let path = changed_entry.path.clone();
                let entry_path = to_entry.get_name().cloned();

                let substream = contents
                    .map(move |(to_content, from_content)| {
                        let to_manifest = get_tree_content(to_content);
                        let from_manifest = get_tree_content(from_content);

                        diff_manifests(
                            MPath::join_element_opt(path.as_ref(), entry_path.as_ref()),
                            &to_manifest,
                            &from_manifest,
                        )
                        .map(recursive_changed_entry_stream)
                    })
                    .flatten_stream()
                    .flatten();

                substream.boxify()
            } else {
                empty().boxify()
            };

            let current_entry = once(Ok(ChangedEntry::new_modified(
                changed_entry.path.clone(),
                to_entry,
                from_entry,
            )));
            current_entry.chain(substream).boxify()
        }
    }
}

/// Given an entry and path from the root of the repo to this entry, returns all subentries with
/// their path from the root of the repo.
/// For a non-tree entry returns a stream with a single (entry, path) pair.
pub fn recursive_entry_stream(
    rootpath: Option<MPath>,
    entry: RevlogEntry,
) -> BoxStream<(Option<MPath>, RevlogEntry), Error> {
    let subentries = match entry.get_type() {
        Type::File(_) => empty().boxify(),
        Type::Tree => {
            let entry_basename = entry.get_name();
            let path = MPath::join_opt(rootpath.as_ref(), entry_basename);

            entry
                .get_content()
                .map(|content| {
                    get_tree_content(content)
                        .list()
                        .map(move |entry| recursive_entry_stream(path.clone(), entry))
                })
                .flatten_stream()
                .flatten()
                .boxify()
        }
    };

    once(Ok((rootpath, entry))).chain(subentries).boxify()
}

/// Difference between manifests, non-recursive.
/// It fetches manifest content, sorts it and compares.
fn diff_manifests(
    path: Option<MPath>,
    to: &RevlogManifest,
    from: &RevlogManifest,
) -> BoxStream<ChangedEntry, Error> {
    let to_vec_future = to.list().collect();
    let from_vec_future = from.list().collect();

    to_vec_future
        .join(from_vec_future)
        .map(|(to, from)| iter_ok(diff_sorted_vecs(path, to, from).into_iter()))
        .flatten_stream()
        .boxify()
}

/// Compares vectors of entries and returns the difference
// TODO(stash): T25644857 this method is made public to make it possible to test it.
// Otherwise we need create dependency to mercurial_types_mocks, which depends on mercurial_types.
// This causing compilation failure.
// We need to find a workaround for an issue.
pub fn diff_sorted_vecs(
    path: Option<MPath>,
    to: Vec<RevlogEntry>,
    from: Vec<RevlogEntry>,
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
                        if to_entry.get_hash() != from_entry.get_hash() {
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

fn get_tree_content(content: EntryContent) -> RevlogManifest {
    match content {
        EntryContent::Tree(manifest) => manifest,
        _ => panic!("Tree entry was expected"),
    }
}
