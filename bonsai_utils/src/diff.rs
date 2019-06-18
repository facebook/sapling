// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Diffing Mercurial changesets to produce something suitable for the Bonsai format.

use std::cmp::Ordering;
use std::fmt;

use crate::failure::Error;
use futures::{
    future::{self, Either},
    stream, Future, Stream,
};
use itertools::{EitherOrBoth, Itertools};

use context::CoreContext;
use futures_ext::{select_all, BoxStream, StreamExt};
use mercurial_types::manifest::{Content, EmptyManifest};
use mercurial_types::{Entry, HgFileNodeId, Manifest, Type};
use mononoke_types::{FileType, MPath};

use crate::composite::CompositeEntry;

/// Compute a list of changed files suitable for the bonsai format. This is path-conflict-free,
/// which means that no returned path that isn't deleted is a prefix of another.
///
/// Items may be returned in arbitrary order.
pub fn bonsai_diff(
    ctx: CoreContext,
    root_entry: Box<dyn Entry + Sync>,
    p1_entry: Option<Box<dyn Entry + Sync>>,
    p2_entry: Option<Box<dyn Entry + Sync>>,
) -> impl Stream<Item = BonsaiDiffResult, Error = Error> + Send {
    let mut composite_entry = CompositeEntry::new();
    if let Some(entry) = p1_entry {
        composite_entry.add_parent(entry);
    }
    if let Some(entry) = p2_entry {
        composite_entry.add_parent(entry);
    }

    WorkingEntry::new(root_entry).bonsai_diff_tree(ctx, None, composite_entry)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BonsaiDiffResult {
    /// This file was changed (was added or modified) in this changeset.
    Changed(MPath, FileType, HgFileNodeId),
    /// The file was marked changed, but one of the parent file node IDs was reused. This can
    /// happen in these situations:
    ///
    /// 1. The file type was changed without a corresponding change in file contents.
    /// 2. There's a merge and one of the parent nodes was picked as the resolution.
    ///
    /// This is separate from `Changed` because in these instances, if copy information is part
    /// of the node it wouldn't be recorded.
    ChangedReusedId(MPath, FileType, HgFileNodeId),
    /// This file was deleted in this changeset.
    Deleted(MPath),
}

impl BonsaiDiffResult {
    /// The path this result refers to.
    #[inline]
    pub fn path(&self) -> &MPath {
        match self {
            BonsaiDiffResult::Changed(path, ..)
            | BonsaiDiffResult::ChangedReusedId(path, ..)
            | BonsaiDiffResult::Deleted(path) => path,
        }
    }
}

impl fmt::Display for BonsaiDiffResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BonsaiDiffResult::Changed(path, ft, entry_id) => write!(
                f,
                "[changed] path: {}, hash: {}, type: {}",
                path, entry_id, ft
            ),
            BonsaiDiffResult::ChangedReusedId(path, ft, entry_id) => write!(
                f,
                "[changed, reused id] path: {}, hash: {}, type: {}",
                path, entry_id, ft
            ),
            BonsaiDiffResult::Deleted(path) => write!(f, "[deleted] path: {}", path),
        }
    }
}

/// This custom implementation of PartialOrd sorts results in their natural order
impl PartialOrd for BonsaiDiffResult {
    #[inline]
    fn partial_cmp(&self, other: &BonsaiDiffResult) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BonsaiDiffResult {
    fn cmp(&self, other: &BonsaiDiffResult) -> Ordering {
        match self.path().cmp(other.path()) {
            Ordering::Equal => DiffResultCmp::from(self).cmp(&DiffResultCmp::from(other)),
            other => other,
        }
    }
}

// (seems like this is the easiest way to do the Ord comparisons necessary)
#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum DiffResultCmp<'a> {
    Changed(&'a FileType, &'a HgFileNodeId),
    ChangedReusedId(&'a FileType, &'a HgFileNodeId),
    Deleted,
}

impl<'a> From<&'a BonsaiDiffResult> for DiffResultCmp<'a> {
    fn from(result: &'a BonsaiDiffResult) -> Self {
        match result {
            BonsaiDiffResult::Changed(_, ft, entry_id) => DiffResultCmp::Changed(ft, entry_id),
            BonsaiDiffResult::ChangedReusedId(_, ft, entry_id) => {
                DiffResultCmp::ChangedReusedId(ft, entry_id)
            }
            BonsaiDiffResult::Deleted(_) => DiffResultCmp::Deleted,
        }
    }
}

/// Represents a specific entry, or the lack of one, in the working manifest.
enum WorkingEntry {
    Absent,
    File(FileType, Box<dyn Entry + Sync>),
    Tree(Box<dyn Entry + Sync>),
}

impl WorkingEntry {
    #[inline]
    fn new(entry: Box<dyn Entry + Sync>) -> Self {
        match entry.get_type() {
            Type::File(ft) => WorkingEntry::File(ft, entry),
            Type::Tree => WorkingEntry::Tree(entry),
        }
    }

    #[inline]
    fn absent() -> Self {
        WorkingEntry::Absent
    }

    #[inline]
    fn manifest(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = Box<dyn Manifest + Sync + 'static>, Error = Error> + Send {
        match self {
            WorkingEntry::Tree(entry) => {
                Either::A(entry.get_content(ctx).map(|content| match content {
                    Content::Tree(mf) => mf,
                    _ => unreachable!("tree entries can only return manifests"),
                }))
            }
            _other => Either::B(future::ok(EmptyManifest::new().boxed())),
        }
    }

    /// The path here corresponds to the path associated with this working entry.
    fn bonsai_diff(
        self,
        ctx: CoreContext,
        path: MPath,
        composite_entry: CompositeEntry,
    ) -> impl Stream<Item = BonsaiDiffResult, Error = Error> + Send {
        let file_result = self.bonsai_diff_file(&path, &composite_entry);
        let tree_stream = match &file_result {
            Some(BonsaiDiffResult::Changed(..)) | Some(BonsaiDiffResult::ChangedReusedId(..)) => {
                // A changed entry automatically means any entries underneath are deleted.
                stream::empty().boxify()
            }
            Some(BonsaiDiffResult::Deleted(..)) | None => {
                self.bonsai_diff_tree(ctx, Some(path), composite_entry)
            }
        };
        let file_stream = stream::iter_ok(file_result);
        // Fetching the composite and working manifests will be kicked off immediately in
        // bonsai_diff_tree, but creating the sub_streams in there won't happen until
        // file_stream is exhausted and tree_stream is polled.
        file_stream.chain(tree_stream)
    }

    /// The path here corresponds to the path associated with this working entry.
    fn bonsai_diff_file(
        &self,
        path: &MPath,
        composite_entry: &CompositeEntry,
    ) -> Option<BonsaiDiffResult> {
        match self {
            WorkingEntry::File(ft, entry) => {
                let (_, hash) = entry.get_hash().to_filenode().expect("filenode expected");
                // Any tree entries being present indicates a file-directory conflict which must be
                // resolved.
                // >= 2 entries means there's a file-file conflict which must be resolved.
                // 0 entries means an added file.
                // A different entry means a changed file.
                //
                // See the doc comment for `BonsaiDiffResult::ChangedReusedId` for more about it.
                if composite_entry.num_trees() == 0 && composite_entry.num_files() == 1 {
                    if composite_entry.contains_file(ft, hash) {
                        None
                    } else if composite_entry.contains_file_other_type(ft, hash) {
                        Some(BonsaiDiffResult::ChangedReusedId(path.clone(), *ft, hash))
                    } else {
                        Some(BonsaiDiffResult::Changed(path.clone(), *ft, hash))
                    }
                } else if composite_entry.contains_file_any_type(hash) {
                    Some(BonsaiDiffResult::ChangedReusedId(path.clone(), *ft, hash))
                } else {
                    Some(BonsaiDiffResult::Changed(path.clone(), *ft, hash))
                }
            }
            _other => {
                // tree or missing -- mark deleted if any files exist
                if composite_entry.num_files() != 0 {
                    Some(BonsaiDiffResult::Deleted(path.clone()))
                } else {
                    None
                }
            }
        }
    }

    /// The path here corresponds to the path associated with the working entry. The only
    /// difference is that self can also be the root entry here but not in the other
    /// methods.
    fn bonsai_diff_tree(
        self,
        ctx: CoreContext,
        path: Option<MPath>,
        composite_entry: CompositeEntry,
    ) -> BoxStream<BonsaiDiffResult, Error> {
        // The return type must be BoxStream because otherwise rustc complains with error E0275:
        // overflow evaluating the requirement `impl std::marker::Send+futures::Stream`. That's
        // probably because there are a bunch of mutually recursive functions here.

        // >= 2 tree entries means there's a directory-directory conflict which must be resolved.
        // 0 entries means this is a directory that was added, so recurse.
        // If working_entry is a tree and the number of tree entries in the composite entry is 1,
        // the number of file entries could be:
        // (a) 0 if there are no conflicting files -- this is the usual case.
        // (b) >=1, which can only happen when a file-directory conflict was resolved in favor of
        // the directory. That's the case where the file gets marked deleted -- no need to recurse
        // into the directory if the hash is the same.
        if let WorkingEntry::Tree(entry) = &self {
            let hash = entry.get_hash().to_manifest().expect("manifest expected");
            if composite_entry.num_trees() == 1 && composite_entry.contains_tree(hash) {
                return stream::empty().boxify();
            }
        }

        let working_mf_fut = self.manifest(ctx.clone());
        composite_entry
            .manifest(ctx.clone())
            .join(working_mf_fut)
            .map(move |(composite_mf, working_mf)| {
                let sub_streams = composite_mf
                    .into_iter()
                    .merge_join_by(working_mf.list(), |(cname, _), wentry| {
                        let wname = wentry
                            .get_name()
                            .expect("manifest entries should have names");
                        cname.cmp(wname)
                    })
                    .map(move |entry_pair| {
                        match entry_pair {
                            EitherOrBoth::Left((name, centry)) => {
                                // This entry was removed from the working set.
                                let sub_path = MPath::join_opt_element(path.as_ref(), &name);
                                WorkingEntry::absent().bonsai_diff(ctx.clone(), sub_path, centry)
                            }
                            EitherOrBoth::Right(wentry) => {
                                // This entry was added to the working set.
                                let sub_path = {
                                    let name = wentry
                                        .get_name()
                                        .expect("manifest entries should have names");
                                    MPath::join_opt_element(path.as_ref(), name)
                                };
                                WorkingEntry::new(wentry).bonsai_diff(
                                    ctx.clone(),
                                    sub_path,
                                    CompositeEntry::new(),
                                )
                            }
                            EitherOrBoth::Both((name, centry), wentry) => {
                                // This entry is present in both the working set and at least one of
                                // the parents.
                                let sub_path = MPath::join_opt_element(path.as_ref(), &name);
                                WorkingEntry::new(wentry).bonsai_diff(ctx.clone(), sub_path, centry)
                            }
                        }
                    });
                // Using select_all as opposed to flatten allows diffs to run in parallel.
                select_all(sub_streams)
            })
            .flatten_stream()
            .boxify()
    }
}
