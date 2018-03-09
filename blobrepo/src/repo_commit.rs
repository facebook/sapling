// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem;
use std::sync::{Arc, Mutex};

use failure::Compat;
use futures::future::{self, Future, Shared, SharedError, SharedItem};
use futures::stream::Stream;
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, FutureExt};

use blobstore::Blobstore;
use linknodes::{ErrorKind as LinknodeErrorKind, Linknodes};
use mercurial::changeset::RevlogChangeset;
use mercurial_types::{Changeset, Entry, EntryId, MPath, Manifest, NodeHash, Parents, RepoPath,
                      Time};
use mercurial_types::manifest::{self, Content};
use mercurial_types::manifest_utils::{changed_entry_stream, EntryStatus};
use mercurial_types::nodehash::ManifestId;

use BlobChangeset;
use BlobRepo;
use errors::*;
use file::BlobEntry;
use utils::get_node_key;

/// A handle to a possibly incomplete BlobChangeset. This is used instead of
/// Future<Item = BlobChangeset> where we don't want to fully serialize waiting for completion.
/// For example, `create_changeset` takes these as p1/p2 so that it can handle the blobstore side
/// of creating a new changeset before its parent changesets are complete.
/// See `get_completed_changeset()` for the public API you can use to extract the final changeset
#[derive(Clone)]
pub struct ChangesetHandle {
    can_be_parent: Shared<oneshot::Receiver<(NodeHash, ManifestId)>>,
    completion_future: Shared<BoxFuture<BlobChangeset, Compat<Error>>>,
}

impl ChangesetHandle {
    pub fn new_pending(
        can_be_parent: Shared<oneshot::Receiver<(NodeHash, ManifestId)>>,
        completion_future: Shared<BoxFuture<BlobChangeset, Compat<Error>>>,
    ) -> Self {
        Self {
            can_be_parent,
            completion_future,
        }
    }

    pub fn get_completed_changeset(self) -> Shared<BoxFuture<BlobChangeset, Compat<Error>>> {
        self.completion_future
    }
}

impl From<BlobChangeset> for ChangesetHandle {
    fn from(bcs: BlobChangeset) -> Self {
        let (trigger, can_be_parent) = oneshot::channel();
        // The send cannot fail at this point, barring an optimizer noticing that `can_be_parent`
        // is unused and dropping early. Eat the error, as in this case, nothing is blocked waiting
        // for the send
        let _ = trigger.send((bcs.get_changeset_id().into_nodehash(), *bcs.manifestid()));
        Self {
            can_be_parent: can_be_parent.shared(),
            completion_future: future::ok(bcs).boxify().shared(),
        }
    }
}

/// State used while tracking uploaded entries, to ensure that a changeset ends up with the right
/// set of blobs uploaded, and all linknodes present.
struct UploadEntriesState {
    /// Listing of blobs that we need, based on parsing the root manifest and all the newly
    /// uploaded child manifests
    required_entries: HashMap<RepoPath, EntryId>,
    /// All the blobs that have been uploaded in this changeset
    uploaded_entries: HashMap<RepoPath, EntryId>,
    /// Parent hashes (if any) of the blobs that have been uploaded in this changeset. Used for
    /// validation of this upload - all parents must either have been uploaded in this changeset,
    /// or be present in the blobstore before the changeset can complete.
    parents: HashSet<NodeHash>,
    blobstore: Arc<Blobstore>,
}

#[derive(Clone)]
pub struct UploadEntries {
    inner: Arc<Mutex<UploadEntriesState>>,
}

impl UploadEntries {
    pub fn new(blobstore: Arc<Blobstore>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(UploadEntriesState {
                required_entries: HashMap::new(),
                uploaded_entries: HashMap::new(),
                parents: HashSet::new(),
                blobstore,
            })),
        }
    }

    /// Parse a manifest and record the referenced blobs so that we know whether or not we have
    /// a complete changeset with all blobs, or whether there is missing data.
    fn process_manifest(&self, entry: &BlobEntry, path: RepoPath) -> BoxFuture<(), Error> {
        let inner_mutex = self.inner.clone();
        let parents_found = self.find_parents(entry);
        let entry_hash = entry.get_hash().into_nodehash();
        let entry_type = entry.get_type();

        entry
            .get_content()
            .and_then(move |content| match content {
                Content::Tree(manifest) => manifest
                    .list()
                    .for_each(move |entry| {
                        let mpath = path.mpath()
                            .unwrap_or(&MPath::empty())
                            .join_element(entry.get_name());
                        let path = try_boxfuture!(match entry.get_type() {
                            manifest::Type::File
                            | manifest::Type::Symlink
                            | manifest::Type::Executable => RepoPath::file(mpath),
                            manifest::Type::Tree => RepoPath::dir(mpath),
                        });
                        let mut inner = inner_mutex.lock().expect("Lock poisoned");
                        inner.required_entries.insert(path, *entry.get_hash());
                        future::ok(()).boxify()
                    })
                    .boxify(),
                _ => {
                    return future::err(ErrorKind::NotAManifest(entry_hash, entry_type).into())
                        .boxify()
                }
            })
            .join(parents_found)
            .map(|_| ())
            .boxify()
    }

    fn find_parents(&self, entry: &BlobEntry) -> BoxFuture<(), Error> {
        let inner_mutex = self.inner.clone();
        entry
            .get_parents()
            .and_then(move |parents| {
                let mut inner = inner_mutex.lock().expect("Lock poisoned");
                inner.parents.extend(parents.into_iter());

                future::ok(())
            })
            .map(|_| ())
            .boxify()
    }

    /// The root manifest needs special processing - unlike all other entries, it is required even
    /// if no other manifest references it. Otherwise, this function is the same as
    /// `process_one_entry` and can be called after it.
    /// It is safe to call this multiple times, but not recommended - every manifest passed to
    /// this function is assumed required for this commit, even if it is not the root.
    pub fn process_root_manifest(&self, entry: &BlobEntry) -> BoxFuture<(), Error> {
        if entry.get_type() != manifest::Type::Tree {
            return future::err(
                ErrorKind::NotAManifest(entry.get_hash().into_nodehash(), entry.get_type()).into(),
            ).boxify();
        }
        {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            inner
                .required_entries
                .insert(RepoPath::root(), *entry.get_hash());
        }
        self.process_one_entry(entry, RepoPath::root())
    }

    pub fn process_one_entry(&self, entry: &BlobEntry, path: RepoPath) -> BoxFuture<(), Error> {
        {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            inner
                .uploaded_entries
                .insert(path.clone(), *entry.get_hash());
        }
        if entry.get_type() == manifest::Type::Tree {
            self.process_manifest(entry, path)
        } else {
            self.find_parents(&entry)
        }
    }

    pub fn finalize(self, linknodes: Arc<Linknodes>, cs_id: NodeHash) -> BoxFuture<(), Error> {
        let required_checks = {
            let inner = self.inner.lock().expect("Lock poisoned");
            let checks: Vec<_> = inner
                .required_entries
                .iter()
                .filter_map(|(path, entryid)| {
                    if inner.uploaded_entries.contains_key(path) {
                        None
                    } else {
                        let key = get_node_key(entryid.into_nodehash());
                        let blobstore = inner.blobstore.clone();
                        Some(blobstore.assert_present(key))
                    }
                })
                .collect();

            future::join_all(checks).boxify()
        };

        let parent_checks = {
            let inner = self.inner.lock().expect("Lock poisoned");
            let checks: Vec<_> = inner
                .parents
                .iter()
                .map(|nodeid| {
                    let key = get_node_key(*nodeid);
                    let blobstore = inner.blobstore.clone();
                    blobstore.assert_present(key)
                })
                .collect();

            future::join_all(checks).boxify()
        };

        let linknodes = {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            let uploaded_entries = mem::replace(&mut inner.uploaded_entries, HashMap::new());
            let futures = uploaded_entries.into_iter().map(move |(path, entryid)| {
                linknodes
                    .add(path, &entryid.into_nodehash(), &cs_id)
                    .or_else(|err| match err.downcast_ref::<LinknodeErrorKind>() {
                        Some(&LinknodeErrorKind::AlreadyExists { .. }) => future::ok(()),
                        _ => future::err(err),
                    })
            });
            future::join_all(futures).boxify()
        };

        parent_checks
            .join3(required_checks, linknodes)
            .map(|_| ())
            .boxify()
    }
}

fn compute_changed_files_pair(
    to: &Box<Manifest + Sync>,
    from: &Box<Manifest + Sync>,
) -> BoxFuture<HashSet<MPath>, Error> {
    changed_entry_stream(to, from, MPath::empty())
        .filter_map(|change| match change.status {
            EntryStatus::Deleted(entry)
            | EntryStatus::Added(entry)
            | EntryStatus::Modified(entry, _) => {
                if entry.get_type() == manifest::Type::Tree {
                    None
                } else {
                    Some(change.path.join_element(entry.get_name()))
                }
            }
        })
        .fold(HashSet::new(), |mut set, path| {
            set.insert(path);
            future::ok::<_, Error>(set)
        })
        .boxify()
}

pub fn compute_changed_files(
    root: &Box<Manifest + Sync>,
    p1: Option<&Box<Manifest + Sync>>,
    p2: Option<&Box<Manifest + Sync>>,
) -> BoxFuture<Vec<MPath>, Error> {
    let empty = manifest::EmptyManifest {}.boxed();
    match (p1, p2) {
        (None, None) => compute_changed_files_pair(&root, &empty),
        (Some(manifest), None) | (None, Some(manifest)) => {
            compute_changed_files_pair(&root, &manifest)
        }
        (Some(p1), Some(p2)) => compute_changed_files_pair(&root, &p1)
            .join(compute_changed_files_pair(&root, &p2))
            .map(|(left, right)| {
                left.symmetric_difference(&right)
                    .cloned()
                    .collect::<HashSet<MPath>>()
            })
            .boxify(),
    }.map(|files| {
        let mut files: Vec<MPath> = files.into_iter().collect();
        files.sort_unstable();

        files
    })
        .boxify()
}

pub fn process_entries(
    repo: BlobRepo,
    entry_processor: &UploadEntries,
    root_manifest: BoxFuture<(BlobEntry, RepoPath), Error>,
    new_child_entries: BoxStream<(BlobEntry, RepoPath), Error>,
) -> BoxFuture<(Box<Manifest + Sync>, ManifestId), Error> {
    root_manifest
        .and_then({
            let entry_processor = entry_processor.clone();
            move |(entry, path)| {
                let hash = entry.get_hash().into_nodehash();
                if entry.get_type() == manifest::Type::Tree && path == RepoPath::RootPath {
                    entry_processor
                        .process_root_manifest(&entry)
                        .map(move |_| hash)
                        .boxify()
                } else {
                    future::err(Error::from(ErrorKind::BadRootManifest(entry.get_type()))).boxify()
                }
            }
        })
        .and_then({
            let entry_processor = entry_processor.clone();
            |hash| {
                new_child_entries
                    .for_each(move |(entry, path)| entry_processor.process_one_entry(&entry, path))
                    .map(move |_| hash)
            }
        })
        .and_then(move |root_hash| {
            repo.get_manifest_by_nodeid(&root_hash)
                .map(move |m| (m, ManifestId::new(root_hash)))
        })
        .boxify()
}

pub fn extract_parents_complete(
    p1: &Option<ChangesetHandle>,
    p2: &Option<ChangesetHandle>,
) -> BoxFuture<SharedItem<()>, SharedError<Compat<Error>>> {
    match (p1.as_ref(), p2.as_ref()) {
        (None, None) => future::ok(()).shared().boxify(),
        (Some(p), None) | (None, Some(p)) => p.completion_future
            .clone()
            .and_then(|_| future::ok(()).shared())
            .boxify(),
        (Some(p1), Some(p2)) => p1.completion_future
            .clone()
            .join(p2.completion_future.clone())
            .and_then(|_| future::ok(()).shared())
            .boxify(),
    }.boxify()
}

pub fn handle_parents(
    repo: BlobRepo,
    p1: Option<ChangesetHandle>,
    p2: Option<ChangesetHandle>,
) -> BoxFuture<
    (
        Parents,
        (Option<Box<Manifest + Sync>>),
        (Option<Box<Manifest + Sync>>),
    ),
    Error,
> {
    let p1 = p1.map(|cs| cs.can_be_parent);
    let p2 = p2.map(|cs| cs.can_be_parent);
    p1.join(p2)
        .and_then(|(p1, p2)| {
            let p1 = match p1 {
                Some(item) => {
                    let (hash, manifest) = *item;
                    (Some(hash), Some(manifest))
                }
                None => (None, None),
            };
            let p2 = match p2 {
                Some(item) => {
                    let (hash, manifest) = *item;
                    (Some(hash), Some(manifest))
                }
                None => (None, None),
            };
            future::ok((p1, p2))
        })
        .map_err(|e| Error::from(e))
        .and_then(move |((p1_hash, p1_manifest), (p2_hash, p2_manifest))| {
            let parents = Parents::new(p1_hash.as_ref(), p2_hash.as_ref());
            let p1_manifest = p1_manifest.map(|m| repo.get_manifest_by_nodeid(&m.into_nodehash()));
            let p2_manifest = p2_manifest.map(|m| repo.get_manifest_by_nodeid(&m.into_nodehash()));
            p1_manifest
                .join(p2_manifest)
                .map(move |(p1_manifest, p2_manifest)| (parents, p1_manifest, p2_manifest))
        })
        .boxify()
}

pub fn make_new_changeset(
    parents: Parents,
    root_hash: ManifestId,
    user: String,
    time: Time,
    extra: BTreeMap<Vec<u8>, Vec<u8>>,
    files: Vec<MPath>,
    comments: String,
) -> Result<BlobChangeset> {
    let changeset = RevlogChangeset::new_from_parts(
        parents,
        root_hash,
        user.into_bytes(),
        time,
        extra,
        files,
        comments.into_bytes(),
    );
    BlobChangeset::new(changeset)
}
