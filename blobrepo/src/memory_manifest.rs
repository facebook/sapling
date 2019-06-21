// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! In memory manifests, used to convert Bonsai Changesets to old style

use std::collections::BTreeMap;
use std::fmt::{self, Debug};
use std::io::Write;
use std::sync::{Arc, Mutex};

use cloned::cloned;
use failure_ext::{Error, Result};
use futures::future::{self, Either, Future, IntoFuture};
use futures::stream::{self, Stream};
use futures_ext::{
    bounded_traversal::{bounded_traversal, Iter as BTIter},
    try_boxfuture, BoxFuture, FutureExt,
};
use slog::Logger;

use blob_changeset::RepoBlobstore;
use context::CoreContext;
use mercurial_types::manifest::Content;
use mercurial_types::{
    Entry, HgFileNodeId, HgManifestId, HgNodeHash, MPath, MPathElement, Manifest, RepoPath, Type,
};
use mononoke_types::{FileContents, FileType};

use super::utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
use super::BlobRepo;
use crate::errors::*;
use crate::file::HgBlobEntry;
use crate::manifest::BlobManifest;
use crate::repo::{UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash, UploadHgTreeEntry};

/// An in-memory manifest entry. Clones are *not* separate - they share a single set of changes.
/// This is because futures require ownership, and I don't want to Arc all of this when there's
/// only a small amount of changing data.
#[derive(Clone)]
pub enum MemoryManifestEntry {
    /// A blob already found in the blob store. This cannot be a Tree blob
    Blob(HgBlobEntry),
    /// There are conflicting options here, to be resolved
    /// The vector contains each of the conflicting manifest entries, for use in generating
    /// parents of the final entry when bonsai changeset resolution removes this conflict
    Conflict(Vec<MemoryManifestEntry>),
    /// This entry is an in-memory tree, and will need writing out to finish
    /// resolving the manifests
    MemTree {
        base_manifest_id: Option<HgNodeHash>,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
        changes: Arc<Mutex<BTreeMap<MPathElement, Option<MemoryManifestEntry>>>>,
    },
}

impl Debug for MemoryManifestEntry {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryManifestEntry::Blob(blob) => fmt
                .debug_tuple("Blob hash")
                .field(&blob.get_hash())
                .finish(),
            MemoryManifestEntry::Conflict(conflicts) => {
                fmt.debug_list().entries(conflicts.iter()).finish()
            }
            MemoryManifestEntry::MemTree {
                base_manifest_id,
                p1,
                p2,
                changes,
            } => {
                let changes = changes.lock().expect("lock poisoned");
                fmt.debug_struct("MemTree")
                    .field("base_manifest_id", base_manifest_id)
                    .field("p1", p1)
                    .field("p2", p2)
                    .field("changes", &*changes)
                    .finish()
            }
        }
    }
}

// This is tied to the implementation of MemoryManifestEntry::save below
fn extend_repopath_with_dir(path: &RepoPath, dir: &MPathElement) -> RepoPath {
    assert!(path.is_dir() || path.is_root(), "Cannot extend a filepath");

    let opt_mpath = MPath::join_opt(path.mpath(), dir);
    match opt_mpath {
        None => RepoPath::root(),
        Some(p) => RepoPath::dir(p).expect("Can't convert an MPath to an MPath?!?"),
    }
}

impl MemoryManifestEntry {
    /// True iff this entry is a tree with no children
    pub fn is_empty(
        &self,
        ctx: CoreContext,
        blobstore: &RepoBlobstore,
    ) -> impl Future<Item = bool, Error = Error> + Send {
        match self {
            MemoryManifestEntry::MemTree {
                changes,
                base_manifest_id,
                ..
            } => {
                let changes_are_empty = {
                    let changes = changes.lock().expect("lock poisoned");
                    changes.is_empty()
                };
                if changes_are_empty {
                    Either::B(future::ok(base_manifest_id.is_none()))
                } else {
                    let is_empty_rec = self.get_new_children(ctx.clone(), blobstore)
                    .and_then({
                        cloned!(ctx, blobstore);
                        move |children| {
                            future::join_all(
                                children
                                    .into_iter()
                                    .map(move |(_, child)| child.is_empty(ctx.clone(), &blobstore)),
                            )
                        }
                    })
                    .map(|f| f.into_iter().all(|ce| ce))
                    // Needed because otherwise I get
                    // error[E0275]: overflow evaluating the requirement
                    // `impl std::marker::Send+futures::Future`
                    .boxify();
                    Either::A(is_empty_rec)
                }
            }
            _ => Either::B(future::ok(false)),
        }
    }

    /// True if this entry is a Tree, false otherwise
    pub fn is_dir(&self) -> bool {
        match self {
            MemoryManifestEntry::MemTree { .. } => true,
            _ => false,
        }
    }

    /// Get an empty tree manifest entry
    pub fn empty_tree() -> Self {
        MemoryManifestEntry::MemTree {
            base_manifest_id: None,
            p1: None,
            p2: None,
            changes: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// True if there's been any modification to self, false if not a MemTree or unmodified
    fn is_modified(&self) -> bool {
        if let MemoryManifestEntry::MemTree {
            base_manifest_id,
            changes,
            ..
        } = self
        {
            // We are definitionally modified if there's no baseline,
            // even if we're actually empty
            let changes = changes.lock().expect("lock poisoned");
            base_manifest_id.is_none() || !changes.is_empty()
        } else {
            false
        }
    }

    /// Save all manifests represented here to the blobstore
    pub fn save(
        &self,
        ctx: CoreContext,
        blobstore: &RepoBlobstore,
        logger: &Logger,
        incomplete_filenodes: &IncompleteFilenodes,
        path: RepoPath,
    ) -> impl Future<Item = HgBlobEntry, Error = Error> {
        let unfold = {
            cloned!(ctx, blobstore);
            move |(path, entry): &mut (RepoPath, Self)| match entry {
                MemoryManifestEntry::Blob(_) => future::ok(Vec::new()).left_future(),
                MemoryManifestEntry::Conflict(_) => {
                    future::err(ErrorKind::UnresolvedConflicts.into()).left_future()
                }
                MemoryManifestEntry::MemTree { .. } => {
                    if !entry.is_modified() {
                        return future::ok(Vec::new()).left_future();
                    }
                    cloned!(path);
                    entry
                        .get_new_children(ctx.clone(), &blobstore)
                        .map(move |children| {
                            children
                                .into_iter()
                                .map(|(path_elem, child)| {
                                    (extend_repopath_with_dir(&path, &path_elem), child)
                                })
                                .collect()
                        })
                        .right_future()
                }
            }
        };

        let fold = {
            cloned!(ctx, blobstore, logger, incomplete_filenodes);
            move |(path, entry): (RepoPath, Self), children: BTIter<Option<HgBlobEntry>>| {
                match entry {
                    MemoryManifestEntry::Blob(blob) => future::ok(Some(blob.clone())).left_future(),
                    MemoryManifestEntry::Conflict(_) => {
                        future::err(ErrorKind::UnresolvedConflicts.into()).left_future()
                    }
                    MemoryManifestEntry::MemTree { p1, p2, .. } if entry.is_modified() => {
                        let mut manifest: Vec<u8> = Vec::new();
                        for child in children.flatten() {
                            manifest.extend(
                                child
                                    .get_name()
                                    .expect("root manifest is never part of other manifest")
                                    .as_ref(),
                            );
                            write!(
                                &mut manifest,
                                "\0{}{}\n",
                                child.get_hash().into_nodehash(),
                                child.get_type().manifest_suffix(),
                            )
                            .expect("Writing to memory failed!");
                        }
                        if manifest.is_empty() && !path.is_root() {
                            return future::ok(None).left_future();
                        }
                        UploadHgTreeEntry {
                            upload_node_id: UploadHgNodeHash::Generate,
                            contents: manifest.into(),
                            p1: p1.clone(),
                            p2: p2.clone(),
                            path,
                        }
                        .upload_to_blobstore(ctx.clone(), &blobstore, &logger)
                        .map(|(_hash, future)| future)
                        .into_future()
                        .flatten()
                        .map({
                            cloned!(incomplete_filenodes);
                            move |(entry, path)| {
                                incomplete_filenodes.add(IncompleteFilenodeInfo {
                                    path,
                                    filenode: HgFileNodeId::new(entry.get_hash().into_nodehash()),
                                    p1: p1.map(|h| HgFileNodeId::new(h)),
                                    p2: p2.map(|h| HgFileNodeId::new(h)),
                                    // this is treated as merge so no copyfrom is present
                                    copyfrom: None,
                                });
                                Some(entry)
                            }
                        })
                        .right_future()
                    }
                    MemoryManifestEntry::MemTree {
                        base_manifest_id,
                        p2,
                        ..
                    } => {
                        if p2.is_some() {
                            future::err(ErrorKind::UnchangedManifest.into()).left_future()
                        } else {
                            cloned!(blobstore, base_manifest_id);
                            base_manifest_id
                                .ok_or(ErrorKind::UnchangedManifest.into())
                                .and_then(move |base_manifest_id| {
                                    match path.mpath().map(MPath::basename) {
                                        None => Ok(Some(HgBlobEntry::new_root(
                                            blobstore,
                                            HgManifestId::new(base_manifest_id),
                                        ))),
                                        Some(path) => Ok(Some(HgBlobEntry::new(
                                            blobstore,
                                            path.clone(),
                                            base_manifest_id,
                                            Type::Tree,
                                        ))),
                                    }
                                })
                                .into_future()
                                .left_future()
                        }
                    }
                }
            }
        };

        bounded_traversal(100, (path.clone(), self.clone()), unfold, fold).and_then(move |entry| {
            entry.ok_or_else(move || ErrorKind::SavingEmptyManifest(path).into())
        })
    }

    fn apply_changes(
        changes: Arc<Mutex<BTreeMap<MPathElement, Option<Self>>>>,
        mut children: BTreeMap<MPathElement, Self>,
    ) -> BTreeMap<MPathElement, Self> {
        let changes = changes.lock().expect("lock poisoned");
        for (path, entry) in changes.iter() {
            match entry {
                None => {
                    children.remove(path);
                }
                Some(new) => {
                    children.insert(path.clone(), new.clone());
                }
            }
        }
        children
    }

    // The list of this node's children, or empty if it's not a tree with children.
    fn get_new_children(
        &self,
        ctx: CoreContext,
        blobstore: &RepoBlobstore,
    ) -> impl Future<Item = BTreeMap<MPathElement, Self>, Error = Error> + Send {
        match self {
            MemoryManifestEntry::MemTree {
                base_manifest_id,
                changes,
                ..
            } => match base_manifest_id {
                Some(manifest_id) => Either::B(
                    BlobManifest::load(ctx.clone(), blobstore, HgManifestId::new(*manifest_id))
                        .and_then({
                            let manifest_id = HgManifestId::new(*manifest_id);
                            move |m| m.ok_or(ErrorKind::ManifestMissing(manifest_id).into())
                        })
                        .and_then({
                            cloned!(blobstore);
                            move |m| {
                                let mut children = BTreeMap::new();
                                for entry in m.list() {
                                    let name = entry
                                        .get_name()
                                        .expect("Unnamed entry in a manifest")
                                        .clone();
                                    let memory_entry = match entry.get_type() {
                                        Type::Tree => {
                                            Self::convert_treenode(entry.get_hash().into_nodehash())
                                        }
                                        _ => MemoryManifestEntry::Blob(HgBlobEntry::new(
                                            blobstore.clone(),
                                            name.clone(),
                                            entry.get_hash().into_nodehash(),
                                            entry.get_type(),
                                        )),
                                    };
                                    children.insert(name, memory_entry);
                                }
                                Ok(children)
                            }
                        })
                        .map({
                            let changes = changes.clone();
                            move |children| Self::apply_changes(changes, children)
                        }),
                ),
                // No baseline manifest - take an empty starting point.
                None => Either::A(future::ok(Self::apply_changes(
                    changes.clone(),
                    BTreeMap::new(),
                ))),
            },
            _ => Either::A(future::ok(BTreeMap::new())),
        }
    }

    pub fn convert_treenode(manifest_id: HgNodeHash) -> Self {
        MemoryManifestEntry::MemTree {
            base_manifest_id: Some(manifest_id),
            p1: Some(manifest_id),
            p2: None,
            changes: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn merge_trees(
        ctx: CoreContext,
        mut children: BTreeMap<MPathElement, MemoryManifestEntry>,
        other_children: BTreeMap<MPathElement, MemoryManifestEntry>,
        blobstore: RepoBlobstore,
        logger: Logger,
        incomplete_filenodes: IncompleteFilenodes,
        repo_path: RepoPath,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
    ) -> impl Future<Item = Self, Error = Error> + Send {
        let mut conflicts = stream::FuturesUnordered::new();

        for (path, other_entry) in other_children {
            match children.remove(&path) {
                None => {
                    // Only present in other - take their version.
                    children.insert(path, other_entry);
                }
                Some(conflict_entry) => {
                    // This is safe, because we only save trees to fix conflicts
                    let repo_path = extend_repopath_with_dir(&repo_path, &path);

                    // Remember the conflict for processing later
                    conflicts.push(
                        conflict_entry
                            .merge_with_conflicts(
                                ctx.clone(),
                                other_entry,
                                blobstore.clone(),
                                logger.clone(),
                                incomplete_filenodes.clone(),
                                repo_path,
                            )
                            .map(move |entry| (path, entry)),
                    );
                }
            }
        }

        // Add all the handled conflicts to a MemoryManifestEntry and then make them into a new
        // entry
        conflicts.collect().map(move |conflicts| {
            children.extend(conflicts.into_iter());
            MemoryManifestEntry::MemTree {
                base_manifest_id: None,
                p1,
                p2,
                changes: Arc::new(Mutex::new(
                    children
                        .into_iter()
                        .map(|(path, entry)| (path, Some(entry)))
                        .collect(),
                )),
            }
        })
    }

    /// Merge two MemoryManifests together, tracking conflicts. Conflicts are put in the data
    /// structure in strict order, so that first entry is p1, second is p2 etc.
    pub fn merge_with_conflicts(
        self,
        ctx: CoreContext,
        other: Self,
        blobstore: RepoBlobstore,
        logger: Logger,
        incomplete_filenodes: IncompleteFilenodes,
        repo_path: RepoPath,
    ) -> BoxFuture<Self, Error> {
        use self::MemoryManifestEntry::*;
        if self.is_modified() {
            return self
                .save(
                    ctx.clone(),
                    &blobstore,
                    &logger,
                    &incomplete_filenodes,
                    repo_path.clone(),
                )
                .map(|entry| Self::convert_treenode(entry.get_hash().into_nodehash()))
                .and_then(move |saved| {
                    saved.merge_with_conflicts(
                        ctx,
                        other,
                        blobstore,
                        logger,
                        incomplete_filenodes,
                        repo_path,
                    )
                })
                .boxify();
        }
        if other.is_modified() {
            return other
                .save(
                    ctx.clone(),
                    &blobstore,
                    &logger,
                    &incomplete_filenodes,
                    repo_path.clone(),
                )
                .map(|entry| Self::convert_treenode(entry.get_hash().into_nodehash()))
                .and_then(move |saved| {
                    self.merge_with_conflicts(
                        ctx,
                        saved,
                        blobstore,
                        logger,
                        incomplete_filenodes,
                        repo_path,
                    )
                })
                .boxify();
        }

        match (&self, &other) {
            // Conflicts (on either side) must be resolved before you merge
            (_, Conflict(_)) | (Conflict(_), _) => {
                future::err(ErrorKind::UnresolvedConflicts.into()).boxify()
            }
            // Two identical blobs merge to an unchanged blob
            (Blob(p1), Blob(p2)) if p1 == p2 => future::ok(self.clone()).boxify(),
            // Otherwise, blobs are in conflict - either another blob, or a tree
            (Blob(_), _) | (_, Blob(_)) => {
                future::ok(Conflict(vec![self.clone(), other.clone()])).boxify()
            }
            // If either tree is already a merge, we can't merge further
            (
                MemTree {
                    p1: Some(p1),
                    p2: Some(p2),
                    ..
                },
                _,
            )
            | (
                _,
                MemTree {
                    p1: Some(p1),
                    p2: Some(p2),
                    ..
                },
            ) => {
                // It is a serious bug if p1 == p2 here - we have somehow managed to have the same
                // manifest as two different parents. This implies that this function went wrong
                // in the case below where it merges two manifests
                assert!(p1 != p2);
                future::err(ErrorKind::ManifestAlreadyAMerge(*p1, *p2).into()).boxify()
            }
            (
                MemTree {
                    base_manifest_id: my_id,
                    p1,
                    changes: my_changes,
                    ..
                },
                MemTree {
                    base_manifest_id: other_id,
                    p1: p2,
                    changes: other_changes,
                    ..
                },
            ) => {
                let my_changes = my_changes.lock().expect("lock poisoned");
                let other_changes = other_changes.lock().expect("lock poisoned");
                // Two identical manifests, neither one modified
                if my_id.is_some()
                    && my_id == other_id
                    && my_changes.is_empty()
                    && other_changes.is_empty()
                {
                    future::ok(self.clone()).boxify()
                } else {
                    // Otherwise, merge on an entry-by-entry basis
                    self.get_new_children(ctx.clone(), &blobstore)
                        .join(other.get_new_children(ctx.clone(), &blobstore))
                        .and_then({
                            let p1 = p1.clone();
                            let p2 = p2.clone();
                            move |(children, other_children)| {
                                Self::merge_trees(
                                    ctx,
                                    children,
                                    other_children,
                                    blobstore,
                                    logger,
                                    incomplete_filenodes,
                                    repo_path,
                                    p1,
                                    p2,
                                )
                            }
                        })
                        .boxify()
                }
            }
        }
        .boxify()
    }

    // Only for use in find_mut_helper
    fn conflict_to_memtree(&mut self) -> Self {
        let new = if let MemoryManifestEntry::Conflict(conflicts) = self {
            let mut parents = conflicts
                .into_iter()
                .filter_map(|entry| {
                    let modified = entry.is_modified();
                    match entry {
                        MemoryManifestEntry::MemTree {
                            base_manifest_id, ..
                        } if !modified => *base_manifest_id,
                        MemoryManifestEntry::Blob(blob) if blob.get_type() == Type::Tree => {
                            Some(blob.get_hash().into_nodehash())
                        }
                        _ => None,
                    }
                })
                .fuse();
            Some(MemoryManifestEntry::MemTree {
                base_manifest_id: None,
                p1: parents.next(),
                p2: parents.next(),
                changes: Arc::new(Mutex::new(BTreeMap::new())),
            })
        } else {
            None
        };
        if let Some(new) = new {
            *self = new;
        }
        self.clone()
    }

    fn find_mut_helper<'a>(
        changes: &'a mut BTreeMap<MPathElement, Option<Self>>,
        path: MPathElement,
    ) -> Self {
        changes
            .entry(path)
            .or_insert(Some(Self::empty_tree()))
            .get_or_insert_with(Self::empty_tree)
            .conflict_to_memtree()
    }

    fn manifest_lookup(
        manifest: BlobManifest,
        entry_changes: Arc<Mutex<BTreeMap<MPathElement, Option<MemoryManifestEntry>>>>,
        element: MPathElement,
        blobstore: RepoBlobstore,
    ) {
        if let Some(entry) = manifest.lookup(&element) {
            let mut changes = entry_changes.lock().expect("lock poisoned");
            changes.entry(element.clone()).or_insert_with(move || {
                let entry = match entry.get_type() {
                    Type::Tree => Self::convert_treenode(entry.get_hash().into_nodehash()),
                    _ => MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore,
                        element,
                        entry.get_hash().into_nodehash(),
                        entry.get_type(),
                    )),
                };
                Some(entry)
            });
        }
    }

    /// Creates directories as needed to find the element referred to by path
    /// This will be a tree if it's been freshly created, or whatever is in the manifest if it
    /// was present. Returns a None if the path cannot be created (e.g. there's a file part
    /// way through the path)
    pub fn find_mut(
        &self,
        ctx: CoreContext,
        mut path: impl Iterator<Item = MPathElement> + Send + 'static,
        blobstore: RepoBlobstore,
    ) -> BoxFuture<Option<Self>, Error> {
        match path.next() {
            None => future::ok(Some(self.clone())).boxify(),
            Some(element) => {
                // First check to see if I've already got an entry in changes (while locked),
                // and recurse into that entry
                // If not, lookup the entry
                // On fail, put an empty tree in changes
                // On success, put the lookup result in changes and retry
                match self {
                    MemoryManifestEntry::MemTree {
                        base_manifest_id,
                        changes: entry_changes,
                        ..
                    } => {
                        let entry_changes = entry_changes.clone();
                        let element_known = {
                            let changes = entry_changes.lock().expect("lock poisoned");
                            changes.contains_key(&element)
                        };
                        if element_known {
                            future::ok(()).boxify()
                        } else {
                            // Do the lookup in base_manifest_id
                            if let Some(manifest_id) = base_manifest_id {
                                let manifest_id = HgManifestId::new(*manifest_id);
                                BlobManifest::load(ctx.clone(), &blobstore, manifest_id)
                                    .and_then(move |m| {
                                        m.ok_or(ErrorKind::ManifestMissing(manifest_id).into())
                                    })
                                    .map({
                                        let entry_changes = entry_changes.clone();
                                        let element = element.clone();
                                        let blobstore = blobstore.clone();
                                        move |m| {
                                            Self::manifest_lookup(
                                                m,
                                                entry_changes,
                                                element,
                                                blobstore,
                                            )
                                        }
                                    })
                                    .boxify()
                            } else {
                                future::ok(()).boxify()
                            }
                        }
                        .and_then({
                            cloned!(ctx);
                            move |_| {
                                let mut changes = entry_changes.lock().expect("lock poisoned");
                                Self::find_mut_helper(&mut changes, element)
                                    .find_mut(ctx, path, blobstore)
                            }
                        })
                        .boxify()
                    }
                    _ => future::ok(None).boxify(),
                }
            }
        }
    }

    /// Change an entry - remove if None, set if Some(entry)
    pub fn change(&self, element: MPathElement, change: Option<HgBlobEntry>) -> Result<()> {
        use self::MemoryManifestEntry::{Blob, Conflict, MemTree};

        match self {
            &MemTree { ref changes, .. } => {
                let mut changes = changes.lock().expect("lock poisoned");
                let entry = match changes.get(&element) {
                    Some(Some(Conflict(conflict))) => {
                        let mut conflict = conflict.iter();
                        if let (Some(e0), Some(e1)) = (conflict.next(), conflict.next()) {
                            assert!(
                                conflict.next().is_none(),
                                "Only support two manifest conflict"
                            );
                            match (e0, e1) {
                                (Blob(_), tree @ MemTree { .. })
                                | (tree @ MemTree { .. }, Blob(_)) => match change {
                                    None => Some(tree.clone()),
                                    Some(entry) => Some(Blob(entry)),
                                },
                                _ => change.map(|c| Blob(c)),
                            }
                        } else {
                            return Err(ErrorKind::SingleEntryConflict.into());
                        }
                    }
                    _ => change.map(|c| Blob(c)),
                };
                changes.insert(element, entry);
                Ok(())
            }
            _ => Err(ErrorKind::NotADirectory.into()),
        }
    }

    /// Resolve conflicts when blobs point to the same data but have different parents
    pub fn resolve_trivial_conflicts(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        incomplete_filenodes: IncompleteFilenodes,
    ) -> impl Future<Item = (), Error = Error> + Send {
        fn merge_content(
            ctx: CoreContext,
            entries: Vec<HgBlobEntry>,
        ) -> impl Future<Item = Option<(FileType, FileContents)>, Error = Error> + Send {
            if let Some(Type::File(file_type)) = entries.first().map(move |e| e.get_type()) {
                let fut =
                    future::join_all(entries.into_iter().map(move |e| e.get_content(ctx.clone())))
                        .map(move |content| {
                            let mut iter = content.iter();
                            if let Some(first) = iter.next() {
                                if iter.all(|other| match (first, other) {
                                    (Content::File(c0), Content::File(c1))
                                    | (Content::Executable(c0), Content::Executable(c1))
                                    | (Content::Symlink(c0), Content::Symlink(c1)) => c0 == c1,
                                    _ => false,
                                }) {
                                    return match first {
                                        Content::Executable(file_content)
                                        | Content::File(file_content)
                                        | Content::Symlink(file_content) => {
                                            Some((file_type, file_content.clone()))
                                        }
                                        _ => unreachable!(),
                                    };
                                };
                            };
                            None
                        });
                Either::A(fut)
            } else {
                Either::B(future::ok(None))
            }
        }

        fn merge_entries(
            ctx: CoreContext,
            path: Option<MPath>,
            entries: Vec<HgBlobEntry>,
            repo: BlobRepo,
            incomplete_filenodes: IncompleteFilenodes,
        ) -> impl Future<Item = Option<MemoryManifestEntry>, Error = Error> + Send {
            let parents = entries
                .iter()
                .map(|e| e.get_hash().into_nodehash())
                .collect::<Vec<_>>();
            merge_content(ctx.clone(), entries).and_then(move |content| {
                let mut parents = parents.into_iter();
                if let Some((file_type, file_content)) = content {
                    let path = try_boxfuture!(path.ok_or(ErrorKind::EmptyFilePath).into());
                    let p1 = parents.next().map(HgFileNodeId::new);
                    let p2 = parents.next().map(HgFileNodeId::new);
                    assert!(parents.next().is_none(), "Only support two parents");

                    let upload_entry = UploadHgFileEntry {
                        upload_node_id: UploadHgNodeHash::Generate,
                        contents: UploadHgFileContents::RawBytes(file_content.into_bytes()),
                        file_type,
                        p1,
                        p2,
                        path,
                    };
                    let (_, upload_future) = try_boxfuture!(upload_entry.upload(ctx, &repo));
                    upload_future
                        .map(move |(entry, path)| {
                            incomplete_filenodes.add(IncompleteFilenodeInfo {
                                path,
                                filenode: HgFileNodeId::new(entry.get_hash().into_nodehash()),
                                p1,
                                p2,
                                copyfrom: None,
                            });
                            Some(MemoryManifestEntry::Blob(entry))
                        })
                        .boxify()
                } else {
                    future::ok(None).boxify()
                }
            })
        }

        fn resolve_rec(
            ctx: CoreContext,
            path: Option<MPath>,
            node: MemoryManifestEntry,
            repo: BlobRepo,
            incomplete_filenodes: IncompleteFilenodes,
        ) -> BoxFuture<Option<MemoryManifestEntry>, Error> {
            match node {
                MemoryManifestEntry::MemTree { ref changes, .. } => {
                    let resolve_children = {
                        let changes_guard = changes.lock().expect("lock poisoned");
                        changes_guard
                            .iter()
                            .flat_map(|(k, v)| v.clone().map(|v| (k, v)))
                            .map(|(name, child)| {
                                let path = MPath::join_opt(path.as_ref(), name);
                                resolve_rec(
                                    ctx.clone(),
                                    path,
                                    child,
                                    repo.clone(),
                                    incomplete_filenodes.clone(),
                                )
                                .map({
                                    let name = name.clone();
                                    move |v| v.map(|v| (name, v))
                                })
                            })
                            .collect::<Vec<_>>()
                    };
                    future::join_all(resolve_children)
                        .map({
                            let changes = changes.clone();
                            move |updated| {
                                let mut changes_guard = changes.lock().expect("lock poisoned");
                                for (name, child) in updated.into_iter().flat_map(|v| v) {
                                    changes_guard.insert(name, Some(child));
                                }
                                None
                            }
                        })
                        .boxify()
                }
                MemoryManifestEntry::Conflict(conflict) => {
                    // all conflict entries are blob entries
                    let entries = conflict
                        .iter()
                        .map(|node| match node {
                            &MemoryManifestEntry::Blob(ref blob_entry) => Some(blob_entry.clone()),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>();
                    if let Some(entries) = entries {
                        merge_entries(ctx, path, entries, repo, incomplete_filenodes).boxify()
                    } else {
                        future::ok(None).boxify()
                    }
                }
                _ => future::ok(None).boxify(),
            }
        }
        resolve_rec(ctx, None, self.clone(), repo, incomplete_filenodes).map(|_| ())
    }
}

/// An in memory manifest, created from parent manifests (if any)
pub struct MemoryRootManifest {
    repo: BlobRepo,
    root_entry: MemoryManifestEntry,
    incomplete_filenodes: IncompleteFilenodes,
}

impl MemoryRootManifest {
    fn create(
        repo: BlobRepo,
        incomplete_filenodes: IncompleteFilenodes,
        root_entry: MemoryManifestEntry,
    ) -> Self {
        Self {
            repo,
            root_entry,
            incomplete_filenodes,
        }
    }

    fn create_conflict(
        ctx: CoreContext,
        repo: BlobRepo,
        incomplete_filenodes: IncompleteFilenodes,
        p1_root: MemoryManifestEntry,
        p2_root: MemoryManifestEntry,
    ) -> BoxFuture<Self, Error> {
        p1_root
            .merge_with_conflicts(
                ctx,
                p2_root,
                repo.get_blobstore(),
                repo.get_logger(),
                incomplete_filenodes.clone(),
                RepoPath::root(),
            )
            .map(move |root| Self::create(repo, incomplete_filenodes, root))
            .boxify()
    }

    /// Create an in-memory manifest, backed by the given blobstore, and based on mp1 and mp2
    pub fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        incomplete_filenodes: IncompleteFilenodes,
        mp1: Option<HgNodeHash>,
        mp2: Option<HgNodeHash>,
    ) -> BoxFuture<Self, Error> {
        match (mp1, mp2) {
            (None, None) => future::ok(Self::create(
                repo,
                incomplete_filenodes,
                MemoryManifestEntry::empty_tree(),
            ))
            .boxify(),
            (Some(p), None) | (None, Some(p)) => future::ok(Self::create(
                repo,
                incomplete_filenodes,
                MemoryManifestEntry::convert_treenode(p),
            ))
            .boxify(),
            (Some(p1), Some(p2)) => Self::create_conflict(
                ctx,
                repo,
                incomplete_filenodes,
                MemoryManifestEntry::convert_treenode(p1),
                MemoryManifestEntry::convert_treenode(p2),
            ),
        }
    }

    pub fn get_incomplete_filenodes(&self) -> IncompleteFilenodes {
        self.incomplete_filenodes.clone()
    }

    /// Save this manifest to the blobstore, recursing down to ensure that
    /// all child entries are saved and that there are no conflicts.
    /// Note that child entries can be saved even if a parallel tree has conflicts. E.g. if the
    /// manifest contains dir1/file1 and dir2/file2 and dir2 contains a conflict for file2, dir1
    /// can still be saved to the blobstore.
    /// Returns the saved manifest ID
    pub fn save(&self, ctx: CoreContext) -> BoxFuture<HgBlobEntry, Error> {
        self.root_entry
            .save(
                ctx,
                &self.repo.get_blobstore(),
                &self.repo.get_logger(),
                &self.incomplete_filenodes,
                RepoPath::root(),
            )
            .boxify()
    }

    fn find_path(
        &self,
        ctx: CoreContext,
        path: &MPath,
    ) -> (
        impl Future<Item = MemoryManifestEntry, Error = Error> + Send,
        MPathElement,
    ) {
        let (possible_path, filename) = path.split_dirname();
        let target = match possible_path {
            None => Either::A(future::ok(self.root_entry.clone())),
            Some(filepath) => Either::B(
                self.root_entry
                    .find_mut(ctx, filepath.into_iter(), self.repo.get_blobstore())
                    .and_then({
                        let path = path.clone();
                        |entry| entry.ok_or(ErrorKind::PathNotFound(path).into())
                    }),
            ),
        };

        (target, filename.clone())
    }

    /// Apply an add or remove based on whether the change is None (remove) or Some(blobentry) (add)
    pub fn change_entry(
        &self,
        ctx: CoreContext,
        path: &MPath,
        entry: Option<HgBlobEntry>,
    ) -> BoxFuture<(), Error> {
        let (target, filename) = self.find_path(ctx, path);

        target
            .and_then(|target| target.change(filename, entry).into_future())
            .boxify()
    }

    pub fn resolve_trivial_conflicts(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = (), Error = Error> + Send {
        self.root_entry.resolve_trivial_conflicts(
            ctx,
            self.repo.clone(),
            self.incomplete_filenodes.clone(),
        )
    }

    pub fn unittest_root(&self) -> &MemoryManifestEntry {
        &self.root_entry
    }
}

impl Debug for MemoryRootManifest {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.root_entry.fmt(fmt)
    }
}
