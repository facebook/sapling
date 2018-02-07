// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::future::Future;
use futures::stream::{empty, iter_ok, once, Stream};
use futures_ext::{BoxStream, StreamExt};
use std::collections::VecDeque;

use super::{Entry, MPath, MPathElement, Manifest};
use super::manifest::{Content, Type};

use errors::*;

pub enum EntryStatus {
    Added(Box<Entry + Sync>),
    Deleted(Box<Entry + Sync>),
    // Entries should have the same type. Note - we may change it in future to allow
    // (File, Symlink), (Symlink, Executable) etc
    Modified(Box<Entry + Sync>, Box<Entry + Sync>),
}

pub struct ChangedEntry {
    pub path: MPath,
    pub status: EntryStatus,
}

impl ChangedEntry {
    pub fn new_added(path: MPath, entry: Box<Entry + Sync>) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Added(entry),
        }
    }

    pub fn new_deleted(path: MPath, entry: Box<Entry + Sync>) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Deleted(entry),
        }
    }

    pub fn new_modified(path: MPath, left: Box<Entry + Sync>, right: Box<Entry + Sync>) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Modified(left, right),
        }
    }
}

/// Given two manifests, returns a difference between them. Difference is a stream of
/// ChangedEntry, each showing whether a file/directory was added, deleted or modified.
/// Note: Modified entry contains only entries of the same type i.e. if a file was replaced
/// with a directory of the same name, then returned stream will contain Deleted file entry,
/// and Added directory entry. The same applies for executable and symlinks, although we may
/// change it in future
pub fn changed_entry_stream(
    to: Box<Manifest>,
    from: Box<Manifest>,
    path: MPath,
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
        EntryStatus::Modified(left, right) => {
            debug_assert!(left.get_type() == right.get_type());

            let substream = if left.get_type() == Type::Tree {
                let contents = left.get_content().join(right.get_content());
                let path = changed_entry.path.clone();
                let entry_path = left.get_name().clone();

                let substream = contents
                    .map(move |(left_content, right_content)| {
                        let left_manifest = get_tree_content(left_content);
                        let right_manifest = get_tree_content(right_content);

                        diff_manifests(
                            path.join_element(&entry_path),
                            left_manifest,
                            right_manifest,
                        ).map(recursive_changed_entry_stream)
                    })
                    .flatten_stream()
                    .flatten();

                substream.boxify()
            } else {
                empty().boxify()
            };

            let current_entry = once(Ok(ChangedEntry::new_modified(
                changed_entry.path.clone(),
                left,
                right,
            )));
            current_entry.chain(substream).boxify()
        }
    }
}

/// Given an entry and path from the root of the repo to this entry, returns all subentries with
/// their path from the root of the repo.
/// For a non-tree entry returns a stream with a single (entry, path) pair.
pub fn recursive_entry_stream(
    rootpath: MPath,
    entry: Box<Entry + Sync>,
) -> BoxStream<(MPath, Box<Entry + Sync>), Error> {
    let subentries = match entry.get_type() {
        Type::File | Type::Symlink | Type::Executable => empty().boxify(),
        Type::Tree => {
            let entry_basename = entry.get_name();
            let path = rootpath.join(entry_basename);

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
    path: MPath,
    left: Box<Manifest>,
    right: Box<Manifest>,
) -> BoxStream<ChangedEntry, Error> {
    let left_vec_future = left.list().collect();
    let right_vec_future = right.list().collect();

    left_vec_future
        .join(right_vec_future)
        .map(|(left, right)| iter_ok(diff_sorted_vecs(path, left, right).into_iter()))
        .flatten_stream()
        .boxify()
}

/// Compares vectors of entries and returns the difference
// TODO(stash): T25644857 this method is made public to make it possible to test it.
// Otherwise we need create dependency to mercurial_types_mocks, which depends on mercurial_types.
// This causing compilation failure.
// We need to find a workaround for an issue.
pub fn diff_sorted_vecs(
    path: MPath,
    left: Vec<Box<Entry + Sync>>,
    right: Vec<Box<Entry + Sync>>,
) -> Vec<ChangedEntry> {
    let mut left = VecDeque::from(left);
    let mut right = VecDeque::from(right);

    let mut res = vec![];
    loop {
        match (left.pop_front(), right.pop_front()) {
            (Some(left_entry), Some(right_entry)) => {
                let left_path = left_entry
                    .get_name()
                    .clone()
                    .unwrap_or(MPathElement::new(vec![]));
                let right_path = right_entry
                    .get_name()
                    .clone()
                    .unwrap_or(MPathElement::new(vec![]));

                if left_path < right_path {
                    res.push(ChangedEntry::new_added(path.clone(), left_entry));
                    right.push_front(right_entry);
                } else if left_path > right_path {
                    res.push(ChangedEntry::new_deleted(path.clone(), right_entry));
                    left.push_front(left_entry);
                } else {
                    if left_entry.get_type() == right_entry.get_type() {
                        if left_entry.get_hash() != right_entry.get_hash() {
                            res.push(ChangedEntry::new_modified(
                                path.clone(),
                                left_entry,
                                right_entry,
                            ));
                        }
                    } else {
                        res.push(ChangedEntry::new_added(path.clone(), left_entry));
                        res.push(ChangedEntry::new_deleted(path.clone(), right_entry));
                    }
                }
            }

            (Some(left_entry), None) => {
                res.push(ChangedEntry::new_added(path.clone(), left_entry));
            }

            (None, Some(right_entry)) => {
                res.push(ChangedEntry::new_deleted(path.clone(), right_entry));
            }
            (None, None) => {
                break;
            }
        }
    }

    res
}

fn get_tree_content(content: Content) -> Box<Manifest> {
    match content {
        Content::Tree(manifest) => manifest,
        _ => panic!("Tree entry was expected"),
    }
}
