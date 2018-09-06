// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::{Arc, Mutex};

use failure::{err_msg, Compat, Error, FutureFailureErrorExt, Result, StreamFailureErrorExt,
              prelude::*};
use futures::IntoFuture;
use futures::future::{self, Future, Shared, SharedError, SharedItem};
use futures::stream::{self, Stream};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::Timed;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use stats::Timeseries;

use blobstore::Blobstore;
use filenodes::{FilenodeInfo, Filenodes};
use mercurial::file;
use mercurial_types::{Changeset, Entry, HgChangesetId, HgEntryId, HgNodeHash, HgNodeKey,
                      HgParents, MPath, Manifest, RepoPath, RepositoryId, NULL_HASH};
use mercurial_types::manifest::{self, Content};
use mercurial_types::manifest_utils::{changed_entry_stream, EntryStatus};
use mercurial_types::nodehash::{HgFileNodeId, HgManifestId};
use mononoke_types::{BonsaiChangeset, ChangesetId};

use BlobRepo;
use HgBlobChangeset;
use changeset::HgChangesetContent;
use errors::*;
use file::HgBlobEntry;
use repo::{ChangesetMetadata, RepoBlobstore};

define_stats! {
    prefix = "mononoke.blobrepo_commit";
    process_file_entry: timeseries(RATE, SUM),
    process_tree_entry: timeseries(RATE, SUM),
    finalize_required: timeseries(RATE, AVG, SUM),
    finalize_required_found: timeseries(RATE, AVG, SUM),
    finalize_required_uploading: timeseries(RATE, AVG, SUM),
    finalize_parent: timeseries(RATE, AVG, SUM),
    finalize_uploaded: timeseries(RATE, AVG, SUM),
    finalize_uploaded_filenodes: timeseries(RATE, AVG, SUM),
    finalize_uploaded_manifests: timeseries(RATE, AVG, SUM),
    finalize_compute_copy_from_info: timeseries(RATE, SUM),
}

/// A handle to a possibly incomplete HgBlobChangeset. This is used instead of
/// Future<Item = HgBlobChangeset> where we don't want to fully serialize waiting for completion.
/// For example, `create_changeset` takes these as p1/p2 so that it can handle the blobstore side
/// of creating a new changeset before its parent changesets are complete.
/// See `get_completed_changeset()` for the public API you can use to extract the final changeset
#[derive(Clone)]
pub struct ChangesetHandle {
    can_be_parent: Shared<oneshot::Receiver<(ChangesetId, HgNodeHash, HgManifestId)>>,
    // * Shared is required here because a single changeset can have more than one child, and
    //   all of those children will want to refer to the corresponding future for their parents.
    // * The Compat<Error> here is because the error type for Shared (a cloneable wrapper called
    //   SharedError) doesn't implement Fail, and only implements Error if the wrapped type
    //   implements Error.
    completion_future: Shared<BoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>>,
}

impl ChangesetHandle {
    pub fn new_pending(
        can_be_parent: Shared<oneshot::Receiver<(ChangesetId, HgNodeHash, HgManifestId)>>,
        completion_future: Shared<BoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>>,
    ) -> Self {
        Self {
            can_be_parent,
            completion_future,
        }
    }

    pub fn ready_cs_handle(repo: Arc<BlobRepo>, hg_cs: HgChangesetId) -> Self {
        let bonsai_cs = repo.get_bonsai_from_hg(&hg_cs)
            .and_then(move |bonsai_id| {
                bonsai_id.ok_or(ErrorKind::BonsaiMappingNotFound(hg_cs).into())
            })
            .and_then({
                cloned!(repo);
                move |bonsai_id| repo.get_bonsai_changeset(bonsai_id)
            });

        let cs = repo.get_changeset_by_changesetid(&hg_cs);

        let (trigger, can_be_parent) = oneshot::channel();
        let fut = bonsai_cs.join(cs);
        Self {
            can_be_parent: can_be_parent.shared(),
            completion_future: fut.map_err(Error::compat)
                .inspect(move |(bonsai_cs, hg_cs)| {
                    let _ = trigger.send((
                        bonsai_cs.get_changeset_id(),
                        hg_cs.get_changeset_id().into_nodehash(),
                        *hg_cs.manifestid(),
                    ));
                })
                .boxify()
                .shared(),
        }
    }

    pub fn get_completed_changeset(
        self,
    ) -> Shared<BoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>> {
        self.completion_future
    }
}

/// State used while tracking uploaded entries, to ensure that a changeset ends up with the right
/// set of blobs uploaded, and all filenodes present.
struct UploadEntriesState {
    /// Listing of blobs that we need, based on parsing the root manifest and all the newly
    /// uploaded child manifests
    required_entries: HashMap<RepoPath, HgEntryId>,
    /// All the blobs that have been uploaded in this changeset
    uploaded_entries: HashMap<RepoPath, HgBlobEntry>,
    /// Parent hashes (if any) of the blobs that have been uploaded in this changeset. Used for
    /// validation of this upload - all parents must either have been uploaded in this changeset,
    /// or be present in the blobstore before the changeset can complete.
    parents: HashSet<HgNodeKey>,
    blobstore: RepoBlobstore,
    repoid: RepositoryId,
}

#[derive(Clone)]
pub struct UploadEntries {
    scuba_logger: ScubaSampleBuilder,
    inner: Arc<Mutex<UploadEntriesState>>,
}

impl UploadEntries {
    pub fn new(
        blobstore: RepoBlobstore,
        repoid: RepositoryId,
        scuba_logger: ScubaSampleBuilder,
    ) -> Self {
        Self {
            scuba_logger,
            inner: Arc::new(Mutex::new(UploadEntriesState {
                required_entries: HashMap::new(),
                uploaded_entries: HashMap::new(),
                parents: HashSet::new(),
                blobstore,
                repoid,
            })),
        }
    }

    fn scuba_logger(&self) -> ScubaSampleBuilder {
        self.scuba_logger.clone()
    }

    /// Parse a manifest and record the referenced blobs so that we know whether or not we have
    /// a complete changeset with all blobs, or whether there is missing data.
    fn process_manifest(&self, entry: &HgBlobEntry, path: RepoPath) -> BoxFuture<(), Error> {
        let inner_mutex = self.inner.clone();
        let parents_found = self.find_parents(entry, path.clone());
        let entry_hash = entry.get_hash().into_nodehash();
        let entry_type = entry.get_type();

        entry
            .get_content()
            .and_then(move |content| match content {
                Content::Tree(manifest) => {
                    for entry in manifest.list() {
                        let mpath = MPath::join_element_opt(path.mpath(), entry.get_name());
                        let mpath = match mpath {
                            Some(mpath) => mpath,
                            None => {
                                return future::err(err_msg(
                                    "internal error: unexpected empty MPath",
                                )).boxify()
                            }
                        };
                        let path = match entry.get_type() {
                            manifest::Type::File(_) => RepoPath::FilePath(mpath),
                            manifest::Type::Tree => RepoPath::DirectoryPath(mpath),
                        };
                        let mut inner = inner_mutex.lock().expect("Lock poisoned");
                        inner.required_entries.insert(path, *entry.get_hash());
                    }
                    future::ok(()).boxify()
                }
                _ => future::err(ErrorKind::NotAManifest(entry_hash, entry_type).into()).boxify(),
            })
            .join(parents_found)
            .map(|_| ())
            .boxify()
    }

    fn find_parents(&self, entry: &HgBlobEntry, path: RepoPath) -> BoxFuture<(), Error> {
        let inner_mutex = self.inner.clone();
        entry
            .get_parents()
            .and_then(move |parents| {
                let mut inner = inner_mutex.lock().expect("Lock poisoned");
                let node_keys = parents.into_iter().map(move |hash| HgNodeKey {
                    path: path.clone(),
                    hash,
                });
                inner.parents.extend(node_keys);

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
    pub fn process_root_manifest(&self, entry: &HgBlobEntry) -> BoxFuture<(), Error> {
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

    pub fn process_one_entry(&self, entry: &HgBlobEntry, path: RepoPath) -> BoxFuture<(), Error> {
        {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            inner.uploaded_entries.insert(path.clone(), entry.clone());
        }

        let (err_context, fut) = if entry.get_type() == manifest::Type::Tree {
            STATS::process_tree_entry.add_value(1);
            (
                format!(
                    "While processing manifest with id {} and path {}",
                    entry.get_hash(),
                    path
                ),
                self.process_manifest(entry, path),
            )
        } else {
            STATS::process_file_entry.add_value(1);
            (
                format!(
                    "While processing file with id {} and path {}",
                    entry.get_hash(),
                    path
                ),
                self.find_parents(&entry, path),
            )
        };

        fut.chain_err(err_context).from_err().boxify()
    }

    // Check the blobstore to see whether a particular node is present.
    fn assert_in_blobstore(
        blobstore: RepoBlobstore,
        node_id: HgNodeHash,
        is_tree: bool,
    ) -> BoxFuture<(), Error> {
        let key = if is_tree {
            HgManifestId::new(node_id).blobstore_key()
        } else {
            HgFileNodeId::new(node_id).blobstore_key()
        };
        blobstore.assert_present(key)
    }

    pub fn finalize(self, filenodes: Arc<Filenodes>, cs_id: HgNodeHash) -> BoxFuture<(), Error> {
        let required_checks = {
            let inner = self.inner.lock().expect("Lock poisoned");
            let required_len = inner.required_entries.len();

            let checks: Vec<_> = inner
                .required_entries
                .iter()
                .filter_map(|(path, entryid)| {
                    if inner.uploaded_entries.contains_key(path) {
                        None
                    } else {
                        let path = path.clone();
                        let assert = Self::assert_in_blobstore(
                            inner.blobstore.clone(),
                            entryid.into_nodehash(),
                            path.is_tree(),
                        );
                        Some(
                            assert
                                .with_context(move |_| format!("While checking for path: {}", path))
                                .from_err(),
                        )
                    }
                })
                .collect();

            STATS::finalize_required.add_value(required_len as i64);
            STATS::finalize_required_found.add_value((required_len - checks.len()) as i64);
            STATS::finalize_required_uploading.add_value(checks.len() as i64);

            future::join_all(checks).timed({
                let mut scuba_logger = self.scuba_logger();
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Required checks", None);
                    }
                    Ok(())
                }
            })
        };

        let parent_checks = {
            let inner = self.inner.lock().expect("Lock poisoned");
            let checks: Vec<_> = inner
                .parents
                .iter()
                .map(|node_key| {
                    let assert = Self::assert_in_blobstore(
                        inner.blobstore.clone(),
                        node_key.hash,
                        node_key.path.is_tree(),
                    );
                    let node_key = node_key.clone();
                    assert
                        .with_context(move |_| {
                            format!("While checking for a parent node: {}", node_key)
                        })
                        .from_err()
                })
                .collect();

            STATS::finalize_parent.add_value(checks.len() as i64);

            future::join_all(checks).timed({
                let mut scuba_logger = self.scuba_logger();
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Parent checks", None);
                    }
                    Ok(())
                }
            })
        };

        let filenodes = {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            let uploaded_entries = mem::replace(&mut inner.uploaded_entries, HashMap::new());

            let uploaded_filenodes_cnt = uploaded_entries
                .iter()
                .filter(|&(ref path, _)| path.is_file())
                .count();
            let uploaded_manifests_cnt = uploaded_entries
                .iter()
                .filter(|&(ref path, _)| !path.is_file())
                .count();

            STATS::finalize_uploaded.add_value(uploaded_entries.len() as i64);
            STATS::finalize_uploaded_filenodes.add_value(uploaded_filenodes_cnt as i64);
            STATS::finalize_uploaded_manifests.add_value(uploaded_manifests_cnt as i64);

            self.scuba_logger()
                .add("manifests_count", uploaded_manifests_cnt)
                .add("filelogs_count", uploaded_filenodes_cnt)
                .log_with_msg("Size of changeset", None);

            let filenodeinfos =
                stream::futures_unordered(uploaded_entries.into_iter().map(|(path, blobentry)| {
                    blobentry.get_parents().and_then(move |parents| {
                        compute_copy_from_info(&path, &blobentry, &parents).map(move |copyfrom| {
                            let (p1, p2) = parents.get_nodes();
                            FilenodeInfo {
                                path,
                                filenode: HgFileNodeId::new(blobentry.get_hash().into_nodehash()),
                                p1: p1.cloned().map(HgFileNodeId::new),
                                p2: p2.cloned().map(HgFileNodeId::new),
                                copyfrom,
                                linknode: HgChangesetId::new(cs_id),
                            }
                        })
                    })
                })).boxify();

            filenodes
                .add_filenodes(filenodeinfos, &inner.repoid)
                .timed({
                    let mut scuba_logger = self.scuba_logger();
                    move |stats, result| {
                        if result.is_ok() {
                            scuba_logger
                                .add_future_stats(&stats)
                                .log_with_msg("Upload filenodes", None);
                        }
                        Ok(())
                    }
                })
        };

        parent_checks
            .join3(required_checks, filenodes)
            .map(|_| ())
            .boxify()
    }
}

fn compute_copy_from_info(
    path: &RepoPath,
    blobentry: &HgBlobEntry,
    parents: &HgParents,
) -> BoxFuture<Option<(RepoPath, HgFileNodeId)>, Error> {
    let parents = parents.clone();
    match path {
        &RepoPath::FilePath(_) => {
            STATS::finalize_compute_copy_from_info.add_value(1);
            blobentry
                .get_raw_content()
                .and_then({
                    let parents = parents.clone();
                    move |blob| {
                        // XXX this is broken -- parents.get_nodes() will never return
                        // (None, Some(hash)), which is what BlobNode relies on to figure out
                        // whether a node is copied.
                        let (p1, p2) = parents.get_nodes();
                        file::File::new(blob, p1, p2)
                            .copied_from()
                            .map(|copiedfrom| {
                                copiedfrom.map(|(path, node)| {
                                    (RepoPath::FilePath(path), HgFileNodeId::new(node))
                                })
                            })
                    }
                })
                .boxify()
        }
        &RepoPath::RootPath | &RepoPath::DirectoryPath(_) => {
            // No copy information for directories/repo roots
            Ok(None).into_future().boxify()
        }
    }
}

fn compute_changed_files_pair(
    to: &Box<Manifest + Sync>,
    from: &Box<Manifest + Sync>,
) -> BoxFuture<HashSet<MPath>, Error> {
    changed_entry_stream(to, from, None)
        .filter_map(|change| match change.status {
            EntryStatus::Deleted(entry)
            | EntryStatus::Added(entry)
            | EntryStatus::Modified {
                to_entry: entry, ..
            } => {
                if entry.get_type() == manifest::Type::Tree {
                    None
                } else {
                    MPath::join_element_opt(change.dirname.as_ref(), entry.get_name())
                }
            }
        })
        .fold(HashSet::new(), |mut set, path| {
            set.insert(path);
            future::ok::<_, Error>(set)
        })
        .boxify()
}

/// NOTE: To be used only for generating list of files for old, Mercurial format of Changesets.
///
/// This function is used to extract any new files that the given root manifest has provided
/// compared to the provided p1 and p2 parents.
/// A files is considered new when it was not present in neither of parent manifests or it was
/// present, but with a different content.
/// It sorts the returned Vec<MPath> in the order expected by Mercurial.
///
/// TODO(luk): T28626409 this probably deserves a unit tests, but taking into account that Bonsai
/// Changesets might as well make this function obsolete and that I am not familiar with creating
/// mock Manifests I will postpone writing tests for this
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
                left.intersection(&right)
                    .cloned()
                    .collect::<HashSet<MPath>>()
            })
            .boxify(),
    }.map(|files| {
        let mut files: Vec<MPath> = files.into_iter().collect();
        files.sort_unstable_by(mercurial_mpath_comparator);

        files
    })
        .boxify()
}

fn mercurial_mpath_comparator(a: &MPath, b: &MPath) -> ::std::cmp::Ordering {
    a.to_vec().cmp(&b.to_vec())
}

pub fn process_entries(
    repo: BlobRepo,
    entry_processor: &UploadEntries,
    root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    new_child_entries: BoxStream<(HgBlobEntry, RepoPath), Error>,
) -> BoxFuture<(Box<Manifest + Sync>, HgManifestId), Error> {
    let root_manifest_fut = root_manifest
        .context("While uploading root manifest")
        .from_err()
        .and_then({
            let entry_processor = entry_processor.clone();
            move |root_manifest| match root_manifest {
                None => future::ok(None).boxify(),
                Some((entry, path)) => {
                    let hash = entry.get_hash().into_nodehash();
                    if entry.get_type() == manifest::Type::Tree && path == RepoPath::RootPath {
                        entry_processor
                            .process_root_manifest(&entry)
                            .map(move |_| Some(hash))
                            .boxify()
                    } else {
                        future::err(Error::from(ErrorKind::BadRootManifest(entry.get_type())))
                            .boxify()
                    }
                }
            }
        });

    let child_entries_fut = new_child_entries
        .context("While uploading child entries")
        .from_err()
        .map({
            let entry_processor = entry_processor.clone();
            move |(entry, path)| entry_processor.process_one_entry(&entry, path)
        })
        .buffer_unordered(100)
        .for_each(|()| future::ok(()));

    let mut scuba_logger = entry_processor.scuba_logger();
    root_manifest_fut
        .join(child_entries_fut)
        .and_then(move |(root_hash, ())| match root_hash {
            None => future::ok((
                manifest::EmptyManifest.boxed(),
                HgManifestId::new(NULL_HASH),
            )).boxify(),
            Some(root_hash) => repo.get_manifest_by_nodeid(&root_hash)
                .context("While fetching root manifest")
                .from_err()
                .map(move |m| (m, HgManifestId::new(root_hash)))
                .boxify(),
        })
        .timed(move |stats, result| {
            if result.is_ok() {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Upload entries", None);
            }
            Ok(())
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
    mut scuba_logger: ScubaSampleBuilder,
    p1: Option<ChangesetHandle>,
    p2: Option<ChangesetHandle>,
) -> BoxFuture<(HgParents, Vec<HgManifestId>, Vec<ChangesetId>), Error> {
    let p1 = p1.map(|cs| cs.can_be_parent);
    let p2 = p2.map(|cs| cs.can_be_parent);
    p1.join(p2)
        .and_then(|(p1, p2)| {
            let mut bonsai_parents = vec![];
            let p1 = match p1 {
                Some(item) => {
                    let (bonsai_cs_id, hash, manifest) = *item;
                    bonsai_parents.push(bonsai_cs_id);
                    (Some(hash), Some(manifest))
                }
                None => (None, None),
            };
            let p2 = match p2 {
                Some(item) => {
                    let (bonsai_cs_id, hash, manifest) = *item;
                    bonsai_parents.push(bonsai_cs_id);
                    (Some(hash), Some(manifest))
                }
                None => (None, None),
            };
            Ok((p1, p2, bonsai_parents))
        })
        .map_err(|e| Error::from(e))
        .map(
            move |((p1_hash, p1_manifest), (p2_hash, p2_manifest), bonsai_parents)| {
                let parents = HgParents::new(p1_hash.as_ref(), p2_hash.as_ref());
                let mut parent_manifest_hashes = vec![];
                if let Some(p1_manifest) = p1_manifest {
                    parent_manifest_hashes.push(p1_manifest);
                }
                if let Some(p2_manifest) = p2_manifest {
                    parent_manifest_hashes.push(p2_manifest);
                }
                (parents, parent_manifest_hashes, bonsai_parents)
            },
        )
        .timed(move |stats, result| {
            if result.is_ok() {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Wait for parents ready", None);
            }
            Ok(())
        })
        .boxify()
}

pub fn fetch_parent_manifests(
    repo: BlobRepo,
    parent_manifest_hashes: &Vec<HgManifestId>,
) -> BoxFuture<(Option<Box<Manifest + Sync>>, Option<Box<Manifest + Sync>>), Error> {
    let p1_manifest_hash = parent_manifest_hashes.get(0);
    let p2_manifest_hash = parent_manifest_hashes.get(1);
    let p1_manifest = p1_manifest_hash.map({
        cloned!(repo);
        move |m| repo.get_manifest_by_nodeid(&m.into_nodehash())
    });
    let p2_manifest =
        p2_manifest_hash.map(move |m| repo.get_manifest_by_nodeid(&m.into_nodehash()));

    p1_manifest.join(p2_manifest).boxify()
}

pub fn make_new_changeset(
    parents: HgParents,
    root_hash: HgManifestId,
    cs_metadata: ChangesetMetadata,
    files: Vec<MPath>,
) -> Result<HgBlobChangeset> {
    let changeset = HgChangesetContent::new_from_parts(parents, root_hash, cs_metadata, files);
    HgBlobChangeset::new(changeset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mercurial_mpath_comparator() {
        let mut paths = vec![
            "foo/bar/baz/a.test",
            "foo/bar/baz-boo/a.test",
            "foo-faz/bar/baz/a.test",
        ];

        let mut mpaths: Vec<_> = paths
            .iter()
            .map(|path| MPath::new(path).expect("invalid path"))
            .collect();

        {
            mpaths.sort_unstable();
            let result: Vec<_> = mpaths
                .iter()
                .map(|mpath| String::from_utf8(mpath.to_vec()).unwrap())
                .collect();
            assert!(paths == result);
        }

        {
            paths.sort_unstable();
            mpaths.sort_unstable_by(mercurial_mpath_comparator);
            let result: Vec<_> = mpaths
                .iter()
                .map(|mpath| String::from_utf8(mpath.to_vec()).unwrap())
                .collect();
            assert!(paths == result);
        }
    }
}
