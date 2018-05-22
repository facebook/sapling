// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! In memory manifests, used to convert Bonsai Changesets to old style

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use futures::future::{self, Future};
use futures::stream::Stream;
use futures_ext::{BoxFuture, FutureExt};

use slog::Logger;

use mercurial::{HgNodeHash, NodeHashConversion};
use mercurial_types::{DManifestId, DParents, Entry, MPath, MPathElement, Manifest, RepoPath, Type};

use blobstore::Blobstore;
use file::HgBlobEntry;
use repo::{UploadHgEntry, UploadHgNodeHash};

use errors::*;
use manifest::BlobManifest;

/// An in-memory manifest entry
#[derive(Clone)]
enum MemoryManifestEntry {
    /// A blob already found in the blob store. This cannot be a Tree blob
    Blob(HgBlobEntry),
    /// There are conflicting options here, to be resolved
    /// The vector contains each of the conflicting manifest entries, for use in generating
    /// parents of the final entry when bonsai changeset resolution removes this conflict
    Conflict(Vec<MemoryManifestEntry>),
    /// This entry is an in-memory tree, and will need writing out to finish
    /// resolving the manifests
    MemTree {
        children: BTreeMap<MPathElement, MemoryManifestEntry>,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
        changes: BTreeMap<MPathElement, Option<MemoryManifestEntry>>,
    },
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
    /// True if this entry has no children
    pub fn is_empty(&self) -> bool {
        match self {
            MemoryManifestEntry::MemTree {
                children, changes, ..
            } => {
                // If all changes are removes or empty, and all children are either changed or empty
                changes.values().all(|opt| match opt {
                    None => true,
                    Some(entry) => entry.is_empty(),
                })
                    && children
                        .iter()
                        .all(|(path, entry)| changes.contains_key(path) || entry.is_empty())
            }
            _ => false,
        }
    }

    /// True if this entry is a Tree, false otherwise
    #[cfg(test)]
    pub fn is_dir(&self) -> bool {
        match self {
            &MemoryManifestEntry::MemTree { .. } => true,
            _ => false,
        }
    }

    /// Get an empty tree manifest entry
    pub fn empty_tree() -> Self {
        MemoryManifestEntry::MemTree {
            children: BTreeMap::new(),
            p1: None,
            p2: None,
            changes: BTreeMap::new(),
        }
    }

    /// True if there's been any modification to self, false if not a MemTree or unmodified
    fn is_modified(&self) -> bool {
        if let &MemoryManifestEntry::MemTree { ref changes, .. } = self {
            !changes.is_empty()
        } else {
            false
        }
    }

    /// Save all manifests represented here to the blobstore
    pub fn save(
        &self,
        blobstore: &Arc<Blobstore>,
        logger: &Logger,
        path: RepoPath,
    ) -> BoxFuture<HgBlobEntry, Error> {
        match self {
            &MemoryManifestEntry::Blob(ref blob) => future::ok(blob.clone()).boxify(),
            &MemoryManifestEntry::Conflict(_) => {
                future::err(ErrorKind::UnresolvedConflicts.into()).boxify()
            }
            &MemoryManifestEntry::MemTree {
                ref children,
                p1,
                p2,
                ref changes,
            } => {
                if self.is_modified() {
                    // Two things to do:
                    // 1: join_all() the recursive serialization of all entries
                    // 2: Write out a manifest and return its hash.
                    let mut new_children = children.clone();
                    for (path, opt_replacement) in changes.iter() {
                        match opt_replacement {
                            &None => {
                                new_children.remove(path);
                            }
                            &Some(ref replacement) => {
                                new_children.insert(path.clone(), replacement.clone());
                            }
                        }
                    }

                    let futures: Vec<_> = new_children
                        .iter()
                        .filter(|&(_path, child)| !child.is_empty())
                        .map({
                            let blobstore = blobstore.clone();
                            let path = &path;
                            move |(path_elem, entry)| {
                                let path_elem = path_elem.clone();
                                // This is safe, because we only save if we're saving a tree
                                let entry_path = extend_repopath_with_dir(path, &path_elem);
                                entry
                                    .save(&blobstore, logger, entry_path)
                                    .map(move |entry| (path_elem, entry))
                            }
                        })
                        .collect();

                    let entries = future::join_all(futures.into_iter());

                    entries
                        .and_then({
                            let blobstore = blobstore.clone();
                            let logger = logger.clone();
                            move |entries| {
                                let mut manifest: Vec<u8> = Vec::new();
                                entries.iter().for_each(|&(ref path, ref entry)| {
                                    manifest.extend(path.as_bytes());
                                    // Chances of write to memory failing are low enough that this
                                    // should be safe to ignore
                                    let _ = write!(
                                        &mut manifest,
                                        "\0{}{}\n",
                                        entry.get_hash().into_nodehash(),
                                        entry.get_type(),
                                    );
                                });

                                let upload_entry = UploadHgEntry {
                                    upload_nodeid: UploadHgNodeHash::Generate,
                                    raw_content: manifest.into(),
                                    content_type: Type::Tree,
                                    p1,
                                    p2,
                                    path,
                                };

                                let (_hash, future) = try_boxfuture!(
                                    upload_entry.upload_to_blobstore(&blobstore, &logger)
                                );
                                future.map(|(entry, _path)| entry).boxify()
                            }
                        })
                        .boxify()
                } else {
                    if p2.is_some() {
                        future::err(ErrorKind::UnchangedManifest.into()).boxify()
                    } else {
                        let blobstore = blobstore.clone();
                        future::result(p1.ok_or(ErrorKind::UnchangedManifest.into()))
                            .and_then(move |p1| match path.mpath().map(MPath::basename) {
                                None => future::ok(HgBlobEntry::new_root(
                                    blobstore,
                                    DManifestId::new(p1.into_mononoke()),
                                )),
                                Some(path) => future::ok(HgBlobEntry::new(
                                    blobstore,
                                    path.clone(),
                                    p1.into_mononoke(),
                                    Type::Tree,
                                )),
                            })
                            .boxify()
                    }
                }
            }
        }
    }

    /// Create a MemoryManifestEntry from an existing Mercurial tree.
    pub fn convert_treenode(
        blobstore: Arc<Blobstore>,
        manifest_id: &DManifestId,
    ) -> BoxFuture<Self, Error> {
        // This reads in the manifest, keeps it as p1, and converts it to a memory manifest node
        BlobManifest::load(&blobstore, manifest_id)
            .and_then({
                let manifest_id = manifest_id.clone();
                move |m| {
                    future::result(m.ok_or(
                        ErrorKind::ManifestMissing(manifest_id.into_nodehash()).into(),
                    ))
                }
            })
            .and_then({
                let blobstore = blobstore.clone();
                move |m| {
                    m.list()
                        .and_then(move |entry| {
                            let name = entry
                                .get_name()
                                .expect("Unnamed entry in a manifest")
                                .clone();
                            match entry.get_type() {
                                Type::Tree => Self::convert_treenode(
                                    blobstore.clone(),
                                    &DManifestId::new(entry.get_hash().into_nodehash()),
                                ).map(move |entry| (name, entry))
                                    .boxify(),
                                _ => future::ok(MemoryManifestEntry::Blob(HgBlobEntry::new(
                                    blobstore.clone(),
                                    name.clone(),
                                    entry.get_hash().into_nodehash(),
                                    entry.get_type(),
                                ))).map(move |entry| (name, entry))
                                    .boxify(),
                            }
                        })
                        .fold(BTreeMap::new(), move |mut children, (name, entry)| {
                            children.insert(name, entry);
                            future::ok::<_, Error>(children)
                        })
                }
            })
            .map({
                let manifest_id = manifest_id.clone();
                move |children| MemoryManifestEntry::MemTree {
                    children,
                    p1: Some(manifest_id.into_nodehash().into_mercurial()),
                    p2: None,
                    changes: BTreeMap::new(),
                }
            })
            .boxify()
    }

    /// Merge two MemoryManifests together, tracking conflicts. Conflicts are put in the data
    /// structure in strict order, so that first entry is p1, second is p2 etc.
    pub fn merge_with_conflicts(&self, other: &Self) -> Result<Self> {
        use self::MemoryManifestEntry::*;
        // FIXME: Once this code is lazy, we can merge modified manifests with a save/reload cycle
        assert!(!self.is_modified(), "Cannot merge modified manifests");
        assert!(!other.is_modified(), "Cannot merge modified manifests");

        match (self, other) {
            // Conflicts (on either side) must be resolved before you merge
            (_, Conflict(_)) | (Conflict(_), _) => Err(ErrorKind::UnresolvedConflicts.into()),
            // Two identical blobs merge to an unchanged blob
            (Blob(p1), Blob(p2)) if p1 == p2 => Ok(self.clone()),
            // Otherwise, blobs are in conflict - either another blob, or a tree
            (Blob(_), _) | (_, Blob(_)) => Ok(Conflict(vec![self.clone(), other.clone()])),
            // Identical trees are merged as-is
            (
                MemTree {
                    p1: Some(my_id), ..
                },
                MemTree {
                    p1: Some(other_id), ..
                },
            ) if my_id == other_id =>
            {
                Ok(self.clone())
            }
            (
                MemTree {
                    children,
                    p1: my_id,
                    ..
                },
                MemTree {
                    children: other_children,
                    p1: other_id,
                    ..
                },
            ) => {
                // Otherwise, merge on an entry-by-entry basis
                let mut children = children.clone();
                for (path, other_entry) in other_children.iter() {
                    let new_entry = children
                        .remove(&path)
                        .map(|mine| mine.merge_with_conflicts(&other_entry))
                        .unwrap_or(Ok(other_entry.clone()))?;
                    children.insert(path.clone(), new_entry);
                }
                Ok(MemTree {
                    children,
                    p1: *my_id,
                    p2: *other_id,
                    changes: BTreeMap::new(),
                })
            }
        }
    }

    /// Convert self from a Conflict to an empty MemTree, or leave unchanged if not a Conflict
    fn conflict_to_memtree(self) -> Self {
        if let MemoryManifestEntry::Conflict(conflicts) = self {
            let mut parents = conflicts
                .into_iter()
                .filter_map(|entry| match entry {
                    MemoryManifestEntry::MemTree { p1, .. } if !entry.is_modified() => p1,
                    MemoryManifestEntry::Blob(ref blob) if blob.get_type() == Type::Tree => {
                        Some(blob.get_hash().into_nodehash().into_mercurial())
                    }
                    _ => None,
                })
                .fuse();
            MemoryManifestEntry::MemTree {
                children: BTreeMap::new(),
                p1: parents.next(),
                p2: parents.next(),
                changes: BTreeMap::new(),
            }
        } else {
            self
        }
    }

    /// Creates directories as needed to find the element referred to by path
    /// This will be a tree if it's been freshly created, or whatever is in the manifest if it
    /// was present. Returns a None if the path cannot be created (e.g. there's a file part
    /// way through the path)
    pub fn find_mut<'a, I>(&'a mut self, mut path: I) -> Option<&'a mut Self>
    where
        I: Iterator<Item = MPathElement>,
    {
        match path.next() {
            None => Some(self),
            Some(element) => match self {
                &mut MemoryManifestEntry::MemTree {
                    ref children,
                    ref mut changes,
                    ..
                } => {
                    let existing = children
                        .get(&element)
                        .cloned()
                        .unwrap_or_else(Self::empty_tree)
                        .conflict_to_memtree();
                    let new_entry = changes.entry(element).or_insert(Some(existing));
                    match new_entry {
                        &mut None => None,
                        &mut Some(ref mut dir) => dir.find_mut(path),
                    }
                }
                _ => None,
            },
        }
    }

    /// Remove element from this tree manifest
    pub fn remove(&mut self, element: MPathElement) -> Result<()> {
        match self {
            &mut MemoryManifestEntry::MemTree {
                ref mut changes, ..
            } => {
                changes.insert(element, None);
                Ok(())
            }
            _ => Err(ErrorKind::NotADirectory.into()),
        }
    }

    /// Set the given filename to be a known blob that's already in the blob store. No validation
    /// is done here.
    pub fn set(&mut self, element: MPathElement, entry: HgBlobEntry) -> Result<()> {
        match self {
            &mut MemoryManifestEntry::MemTree {
                ref mut changes, ..
            } => {
                changes.insert(element, Some(MemoryManifestEntry::Blob(entry)));
                Ok(())
            }
            _ => Err(ErrorKind::NotADirectory.into()),
        }
    }
}

/// An in memory manifest, created from parent manifests (if any)
pub struct MemoryRootManifest {
    blobstore: Arc<Blobstore>,
    root_entry: MemoryManifestEntry,
}

impl MemoryRootManifest {
    fn create(blobstore: Arc<Blobstore>, root_entry: MemoryManifestEntry) -> Self {
        Self {
            blobstore,
            root_entry,
        }
    }

    fn create_conflict(
        blobstore: Arc<Blobstore>,
        p1_root: MemoryManifestEntry,
        p2_root: MemoryManifestEntry,
    ) -> Result<Self> {
        Ok(Self::create(
            blobstore,
            p1_root.merge_with_conflicts(&p2_root)?,
        ))
    }

    /// Create an in-memory manifest, backed by the given blobstore, and based on mp1 and mp2
    pub fn new(
        blobstore: Arc<Blobstore>,
        mp1: Option<&HgNodeHash>,
        mp2: Option<&HgNodeHash>,
    ) -> BoxFuture<Self, Error> {
        let parents = DParents::new(
            mp1.map(|p| p.into_mononoke()).as_ref(),
            mp2.map(|p| p.into_mononoke()).as_ref(),
        );
        match parents {
            DParents::None => {
                future::ok(Self::create(blobstore, MemoryManifestEntry::empty_tree())).boxify()
            }
            DParents::One(p) => {
                MemoryManifestEntry::convert_treenode(blobstore.clone(), &DManifestId::new(p))
                    .map(move |root_entry| Self::create(blobstore, root_entry))
                    .boxify()
            }
            DParents::Two(p1, p2) => MemoryManifestEntry::convert_treenode(
                blobstore.clone(),
                &DManifestId::new(p1),
            ).join(MemoryManifestEntry::convert_treenode(
                blobstore.clone(),
                &DManifestId::new(p2),
            ))
                .and_then(move |(p1, p2)| future::result(Self::create_conflict(blobstore, p1, p2)))
                .boxify(),
        }
    }

    /// Save this manifest to the blobstore, recursing down to ensure that
    /// all child entries are saved and that there are no conflicts.
    /// Note that child entries can be saved even if a parallel tree has conflicts. E.g. if the
    /// manifest contains dir1/file1 and dir2/file2 and dir2 contains a conflict for file2, dir1
    /// can still be saved to the blobstore.
    /// Returns the saved manifest ID
    pub fn save(self, logger: &Logger) -> BoxFuture<HgBlobEntry, Error> {
        self.root_entry
            .save(&self.blobstore, logger, RepoPath::root())
            .boxify()
    }

    /// Remove a leaf entry. For now, this must be a file node, and we will walk backwards to
    /// remove empty directories. We do not support pruning a whole directory tree at a time.
    pub fn remove(&mut self, path: &MPath) -> Result<()> {
        let (possible_path, filename) = path.split_dirname();
        let target = match possible_path {
            None => Some(&mut self.root_entry),
            Some(filepath) => self.root_entry.find_mut(filepath.into_iter()),
        }.ok_or(ErrorKind::PathNotFound(path.clone()))?;

        target.remove(filename.clone())
    }

    /// Add an entry, based on a blob you've already created outside this module. Missing
    /// directories will be created for you, and if the entry already exists, it will be replaced
    /// unconditionally.
    pub fn set_entry(&mut self, path: &MPath, entry: HgBlobEntry) -> Result<()> {
        let (possible_path, filename) = path.split_dirname();
        let target = match possible_path {
            None => Some(&mut self.root_entry),
            Some(filepath) => self.root_entry.find_mut(filepath.into_iter()),
        }.ok_or(ErrorKind::PathNotFound(path.clone()))?;

        target.set(filename.clone(), entry)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use many_files_dirs;
    use mercurial_types::{DNodeHash, FileType, nodehash::DEntryId};
    use mercurial_types_mocks::nodehash;
    use slog::Discard;

    fn insert_entry(
        tree: &mut MemoryManifestEntry,
        path: MPathElement,
        entry: MemoryManifestEntry,
    ) {
        match tree {
            &mut MemoryManifestEntry::MemTree {
                ref mut changes, ..
            } => {
                changes.insert(path, Some(entry));
            }
            _ => panic!("Inserting into a non-Tree"),
        }
    }

    #[test]
    fn empty_manifest() {
        async_unit::tokio_unit_test(|| {
            let blobstore = many_files_dirs::getrepo(None).get_blobstore();

            // Create an empty memory manifest
            let memory_manifest = MemoryRootManifest::new(blobstore, None, None)
                .wait()
                .expect("Could not create empty manifest");

            if let MemoryManifestEntry::MemTree {
                children,
                p1,
                p2,
                changes,
            } = memory_manifest.root_entry
            {
                assert!(children.is_empty(), "Empty manifest had children");
                assert!(p1.is_none(), "Empty manifest had p1");
                assert!(p2.is_none(), "Empty manifest had p2");
                assert!(changes.is_empty(), "Empty manifest had new entries changed");
            } else {
                panic!("Empty manifest is not a MemTree");
            }
        })
    }

    #[test]
    fn load_manifest() {
        async_unit::tokio_unit_test(|| {
            let blobstore = many_files_dirs::getrepo(None).get_blobstore();

            let manifest_id = DNodeHash::from_static_str(
                "b267a6869fcc39b37741408b5823cc044233201d",
            ).expect("Could not get nodehash")
                .into_mercurial();

            // Load a memory manifest
            let memory_manifest = MemoryRootManifest::new(blobstore, Some(&manifest_id), None)
                .wait()
                .expect("Could not load manifest");

            if let MemoryManifestEntry::MemTree {
                children,
                p1,
                p2,
                changes,
            } = memory_manifest.root_entry
            {
                for (path, entry) in children {
                    match path.as_bytes() {
                        b"1" | b"2" | b"dir1" => {
                            assert!(!entry.is_dir(), "{:?} is not a file", path)
                        }
                        b"dir2" => assert!(entry.is_dir(), "{:?} is not a tree", path),
                        _ => panic!("Unknown path {:?}", path),
                    }
                }
                assert!(
                    p1 == Some(manifest_id),
                    "Loaded manifest had wrong p1 {:?}",
                    p1
                );
                assert!(p2.is_none(), "Loaded manifest had p2");
                assert!(
                    changes.is_empty(),
                    "Loaded (unaltered) manifest has had entries changed"
                );
            } else {
                panic!("Loaded manifest is not a MemTree");
            }
        })
    }

    #[test]
    fn save_manifest() {
        async_unit::tokio_unit_test(|| {
            let repo = many_files_dirs::getrepo(None);
            let blobstore = repo.get_blobstore();
            let logger = Logger::root(Discard, o![]);

            // Create an empty memory manifest
            let mut memory_manifest = MemoryRootManifest::new(blobstore.clone(), None, None)
                .wait()
                .expect("Could not create empty manifest");

            // Add an entry
            let dir_nodehash = DNodeHash::from_static_str(
                "b267a6869fcc39b37741408b5823cc044233201d",
            ).expect("Could not get nodehash");
            let mut children = BTreeMap::new();
            let file1_mpe = MPathElement::new(b"file1".to_vec())
                .expect("file1 is no longer a valid MPathElement");
            children.insert(
                file1_mpe.clone(),
                MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    file1_mpe,
                    dir_nodehash,
                    Type::File(FileType::Regular),
                )),
            );
            let dir = MemoryManifestEntry::MemTree {
                children,
                p1: Some(dir_nodehash.into_mercurial()),
                p2: None,
                changes: BTreeMap::new(),
            };
            let path =
                MPathElement::new(b"dir".to_vec()).expect("dir is no longer a valid MPathElement");
            insert_entry(&mut memory_manifest.root_entry, path.clone(), dir);

            let manifest_id = memory_manifest
                .save(&logger)
                .wait()
                .expect("Could not save manifest");

            let refound = repo.get_manifest_by_nodeid(&manifest_id.get_hash().into_nodehash())
                .and_then(|m| m.lookup(&path))
                .wait()
                .expect("Lookup of entry just saved failed")
                .expect("Just saved entry not present");

            assert_eq!(
                refound.get_hash().into_nodehash(),
                dir_nodehash,
                "directory hash changed"
            );
        })
    }

    #[test]
    fn remove_item() {
        async_unit::tokio_unit_test(|| {
            let repo = many_files_dirs::getrepo(None);
            let blobstore = repo.get_blobstore();
            let logger = Logger::root(Discard, o![]);

            let manifest_id = DNodeHash::from_static_str(
                "b267a6869fcc39b37741408b5823cc044233201d",
            ).expect("Could not get nodehash")
                .into_mercurial();

            let dir2 = MPathElement::new(b"dir2".to_vec()).expect("Can't create MPathElement dir2");

            // Load a memory manifest
            let mut memory_manifest = MemoryRootManifest::new(blobstore, Some(&manifest_id), None)
                .wait()
                .expect("Could not load manifest");

            if let MemoryManifestEntry::MemTree { ref children, .. } = memory_manifest.root_entry {
                assert!(
                    !children.get(&dir2).expect("dir2 is missing").is_empty(),
                    "Bad load"
                );
            } else {
                panic!("Loaded manifest is not a MemTree");
            }

            // Remove a file
            memory_manifest
                .remove(&MPath::new(b"dir2/file_1_in_dir2").expect("Can't create MPath"))
                .expect("Remove failed");

            // Assert that dir2 is now empty, since we've removed the item
            if let MemoryManifestEntry::MemTree { ref changes, .. } = memory_manifest.root_entry {
                assert!(
                    changes
                        .get(&dir2)
                        .expect("dir2 is missing")
                        .clone()
                        .map_or(false, |e| e.is_empty()),
                    "Bad after remove"
                );
                if let &Some(MemoryManifestEntry::MemTree {
                    ref children,
                    ref changes,
                    ..
                }) = changes.get(&dir2).expect("dir2 is missing")
                {
                    assert!(!children.is_empty(), "dir2 has lost its child");
                    assert!(!changes.is_empty(), "dir2 has no change entries");
                    assert!(
                        changes.values().all(Option::is_none),
                        "dir2 has some add entries"
                    );
                }
            } else {
                panic!("Loaded manifest is not a MemTree");
            }

            // And check that dir2 disappears over a save/reload operation
            let manifest_entry = memory_manifest
                .save(&logger)
                .wait()
                .expect("Could not save manifest");

            let refound = repo.get_manifest_by_nodeid(&manifest_entry.get_hash().into_nodehash())
                .and_then(|m| m.lookup(&dir2))
                .wait()
                .expect("Lookup of entry just saved failed");

            assert!(
                refound.is_none(),
                "Found dir2 when we should have deleted it on save"
            );
        })
    }

    #[test]
    fn add_item() {
        async_unit::tokio_unit_test(|| {
            let repo = many_files_dirs::getrepo(None);
            let blobstore = repo.get_blobstore();
            let logger = Logger::root(Discard, o![]);

            let manifest_id = DNodeHash::from_static_str(
                "b267a6869fcc39b37741408b5823cc044233201d",
            ).expect("Could not get nodehash")
                .into_mercurial();

            let new_file = MPathElement::new(b"new_file".to_vec())
                .expect("Can't create MPathElement new_file");

            // Load a memory manifest
            let mut memory_manifest =
                MemoryRootManifest::new(blobstore.clone(), Some(&manifest_id), None)
                    .wait()
                    .expect("Could not load manifest");

            // Add a file
            let nodehash = DNodeHash::from_static_str("b267a6869fcc39b37741408b5823cc044233201d")
                .expect("Could not get nodehash");
            memory_manifest
                .set_entry(
                    &MPath::new(b"new_file").expect("Could not create MPath"),
                    HgBlobEntry::new(
                        blobstore.clone(),
                        new_file.clone(),
                        nodehash,
                        Type::File(FileType::Regular),
                    ),
                )
                .expect("Add failed");

            // And check that new_file persists
            let manifest_entry = memory_manifest
                .save(&logger)
                .wait()
                .expect("Could not save manifest");

            let refound = repo.get_manifest_by_nodeid(&manifest_entry.get_hash().into_nodehash())
                .and_then(|m| m.lookup(&new_file))
                .wait()
                .expect("Lookup of entry just saved failed")
                .expect("new_file did not persist");
            assert_eq!(
                refound.get_hash().into_nodehash(),
                nodehash,
                "nodehash hash changed"
            );
        })
    }

    #[test]
    fn replace_item() {
        async_unit::tokio_unit_test(|| {
            let repo = many_files_dirs::getrepo(None);
            let blobstore = repo.get_blobstore();
            let logger = Logger::root(Discard, o![]);

            let manifest_id = DNodeHash::from_static_str(
                "b267a6869fcc39b37741408b5823cc044233201d",
            ).expect("Could not get nodehash")
                .into_mercurial();

            let new_file = MPathElement::new(b"1".to_vec()).expect("Can't create MPathElement 1");

            // Load a memory manifest
            let mut memory_manifest =
                MemoryRootManifest::new(blobstore.clone(), Some(&manifest_id), None)
                    .wait()
                    .expect("Could not load manifest");

            // Add a file
            let nodehash = DNodeHash::from_static_str("b267a6869fcc39b37741408b5823cc044233201d")
                .expect("Could not get nodehash");
            memory_manifest
                .set_entry(
                    &MPath::new(b"1").expect("Could not create MPath"),
                    HgBlobEntry::new(
                        blobstore.clone(),
                        new_file.clone(),
                        nodehash,
                        Type::File(FileType::Regular),
                    ),
                )
                .expect("Change failed");

            // And check that new_file persists
            let manifest_entry = memory_manifest
                .save(&logger)
                .wait()
                .expect("Could not save manifest");

            let refound = repo.get_manifest_by_nodeid(&manifest_entry.get_hash().into_nodehash())
                .and_then(|m| m.lookup(&new_file))
                .wait()
                .expect("Lookup of entry just saved failed")
                .expect("1 did not persist");
            assert_eq!(
                refound.get_hash().into_nodehash(),
                nodehash,
                "nodehash hash changed"
            );
        })
    }

    #[test]
    fn merge_manifests() {
        async_unit::tokio_unit_test(|| {
            let repo = many_files_dirs::getrepo(None);
            let blobstore = repo.get_blobstore();

            let base = {
                let mut children = BTreeMap::new();
                let shared = MPathElement::new(b"shared".to_vec()).unwrap();
                let base = MPathElement::new(b"base".to_vec()).unwrap();
                let conflict = MPathElement::new(b"conflict".to_vec()).unwrap();
                children.insert(
                    shared.clone(),
                    MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        shared.clone(),
                        nodehash::ONES_HASH,
                        Type::File(FileType::Regular),
                    )),
                );
                children.insert(
                    base.clone(),
                    MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        base.clone(),
                        nodehash::ONES_HASH,
                        Type::File(FileType::Regular),
                    )),
                );
                children.insert(
                    conflict.clone(),
                    MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        conflict.clone(),
                        nodehash::ONES_HASH,
                        Type::File(FileType::Regular),
                    )),
                );
                MemoryManifestEntry::MemTree {
                    children,
                    p1: Some(nodehash::ONES_HASH.into_mercurial()),
                    p2: None,
                    changes: BTreeMap::new(),
                }
            };

            let other = {
                let mut children = BTreeMap::new();
                let shared = MPathElement::new(b"shared".to_vec()).unwrap();
                let other = MPathElement::new(b"other".to_vec()).unwrap();
                let conflict = MPathElement::new(b"conflict".to_vec()).unwrap();
                children.insert(
                    shared.clone(),
                    MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        shared.clone(),
                        nodehash::ONES_HASH,
                        Type::File(FileType::Regular),
                    )),
                );
                children.insert(
                    other.clone(),
                    MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        other.clone(),
                        nodehash::TWOS_HASH,
                        Type::File(FileType::Regular),
                    )),
                );
                children.insert(
                    conflict.clone(),
                    MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        conflict.clone(),
                        nodehash::TWOS_HASH,
                        Type::File(FileType::Regular),
                    )),
                );
                MemoryManifestEntry::MemTree {
                    children,
                    p1: Some(nodehash::TWOS_HASH.into_mercurial()),
                    p2: None,
                    changes: BTreeMap::new(),
                }
            };

            let merged = base.merge_with_conflicts(&other).expect("Failed to merge");

            if let MemoryManifestEntry::MemTree { children, .. } = merged {
                assert_eq!(children.len(), 4, "Should merge to 4 entries");
                if let Some(&MemoryManifestEntry::Blob(ref blob)) =
                    children.get(&MPathElement::new(b"shared".to_vec()).unwrap())
                {
                    assert_eq!(
                        blob.get_hash(),
                        &DEntryId::new(nodehash::ONES_HASH),
                        "Wrong hash for shared"
                    );
                } else {
                    panic!("shared is not a blob");
                }
                if let Some(&MemoryManifestEntry::Blob(ref blob)) =
                    children.get(&MPathElement::new(b"base".to_vec()).unwrap())
                {
                    assert_eq!(
                        blob.get_hash(),
                        &DEntryId::new(nodehash::ONES_HASH),
                        "Wrong hash for base"
                    );
                } else {
                    panic!("base is not a blob");
                }
                if let Some(&MemoryManifestEntry::Blob(ref blob)) =
                    children.get(&MPathElement::new(b"other".to_vec()).unwrap())
                {
                    assert_eq!(
                        blob.get_hash(),
                        &DEntryId::new(nodehash::TWOS_HASH),
                        "Wrong hash for other"
                    );
                } else {
                    panic!("other is not a blob");
                }
                if let Some(&MemoryManifestEntry::Conflict(ref conflicts)) =
                    children.get(&MPathElement::new(b"conflict".to_vec()).unwrap())
                {
                    assert_eq!(conflicts.len(), 2, "Should have two conflicts");
                } else {
                    panic!("conflict did not create a conflict")
                }
            } else {
                panic!("Merge failed to produce a merged tree");
            }
        })
    }
}
