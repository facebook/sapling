/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::{Arc, Mutex};

use cloned::cloned;
use failure_ext::{
    format_err, prelude::*, Compat, Error, FutureFailureErrorExt, Result, StreamFailureErrorExt,
};
use futures::future::{self, ok, Future, Shared, SharedError, SharedItem};
use futures::stream::{self, Stream};
use futures::sync::oneshot;
use futures::IntoFuture;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::Timed;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use stats::Timeseries;
use tracing::{trace_args, Traced};

use ::manifest::{find_intersection_of_diffs, Entry};
use blobstore::Blobstore;
use context::CoreContext;
use filenodes::{FilenodeInfo, Filenodes};
use mercurial_types::{
    blobs::{ChangesetMetadata, HgBlobChangeset, HgBlobEntry, HgBlobEnvelope, HgChangesetContent},
    manifest,
    manifest_utils::{changed_entry_stream, ChangedEntry, EntryStatus},
    nodehash::{HgFileNodeId, HgManifestId},
    Changeset, HgChangesetId, HgEntry, HgManifest, HgNodeHash, HgNodeKey, HgParents, MPath,
    RepoPath, NULL_HASH,
};
use mononoke_types::{self, BonsaiChangeset, ChangesetId, RepositoryId};
use stats::define_stats;

use crate::errors::*;
use crate::BlobRepo;
use repo_blobstore::RepoBlobstore;

define_stats! {
    prefix = "mononoke.blobrepo_commit";
    process_file_entry: timeseries(RATE, SUM),
    process_tree_entry: timeseries(RATE, SUM),
    finalize_required: timeseries(RATE, AVG, SUM),
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
    can_be_parent: Shared<BoxFuture<(ChangesetId, HgNodeHash, HgManifestId), Compat<Error>>>,
    // * Shared is required here because a single changeset can have more than one child, and
    //   all of those children will want to refer to the corresponding future for their parents.
    // * The Compat<Error> here is because the error type for Shared (a cloneable wrapper called
    //   SharedError) doesn't implement Fail, and only implements Error if the wrapped type
    //   implements Error.
    completion_future: Shared<BoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>>,
}

impl ChangesetHandle {
    pub fn new_pending(
        can_be_parent: Shared<BoxFuture<(ChangesetId, HgNodeHash, HgManifestId), Compat<Error>>>,
        completion_future: Shared<BoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>>,
    ) -> Self {
        Self {
            can_be_parent,
            completion_future,
        }
    }

    pub fn ready_cs_handle(ctx: CoreContext, repo: BlobRepo, hg_cs: HgChangesetId) -> Self {
        let bonsai_cs = repo
            .get_bonsai_from_hg(ctx.clone(), hg_cs)
            .and_then(move |bonsai_id| {
                bonsai_id.ok_or(ErrorKind::BonsaiMappingNotFound(hg_cs).into())
            })
            .and_then({
                cloned!(ctx, repo);
                move |bonsai_id| repo.get_bonsai_changeset(ctx, bonsai_id)
            });

        let cs = repo.get_changeset_by_changesetid(ctx, hg_cs);

        let (trigger, can_be_parent) = oneshot::channel();

        let can_be_parent = can_be_parent
            .into_future()
            .map_err(|e| format_err!("can_be_parent: {:?}", e))
            .map_err(Error::compat)
            .boxify()
            .shared();

        let completion_future = bonsai_cs
            .join(cs)
            .map_err(Error::compat)
            .inspect(move |(bonsai_cs, hg_cs)| {
                let _ = trigger.send((
                    bonsai_cs.get_changeset_id(),
                    hg_cs.get_changeset_id().into_nodehash(),
                    hg_cs.manifestid(),
                ));
            })
            .boxify()
            .shared();

        Self {
            can_be_parent,
            completion_future,
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
    /// All the blobs that have been uploaded in this changeset
    uploaded_entries: HashMap<RepoPath, HgBlobEntry>,
    /// Parent hashes (if any) of the blobs that have been uploaded in this changeset. Used for
    /// validation of this upload - all parents must either have been uploaded in this changeset,
    /// or be present in the blobstore before the changeset can complete.
    parents: HashSet<HgNodeKey>,
    blobstore: RepoBlobstore,
    repoid: RepositoryId,
    /// Draft entries do not have their filenodes stored in the filenodes table.
    draft: bool,
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
        draft: bool,
    ) -> Self {
        Self {
            scuba_logger,
            inner: Arc::new(Mutex::new(UploadEntriesState {
                uploaded_entries: HashMap::new(),
                parents: HashSet::new(),
                blobstore,
                repoid,
                draft,
            })),
        }
    }

    fn scuba_logger(&self) -> ScubaSampleBuilder {
        self.scuba_logger.clone()
    }

    /// Parse a manifest and record the referenced blobs so that we know whether or not we have
    /// a complete changeset with all blobs, or whether there is missing data.
    fn process_manifest(
        &self,
        ctx: CoreContext,
        entry: &HgBlobEntry,
        path: RepoPath,
    ) -> BoxFuture<(), Error> {
        if entry.get_type() != manifest::Type::Tree {
            future::err(
                ErrorKind::NotAManifest(entry.get_hash().into_nodehash(), entry.get_type()).into(),
            )
            .boxify()
        } else {
            self.find_parents(ctx.clone(), entry, path.clone())
        }
    }

    fn find_parents(
        &self,
        ctx: CoreContext,
        entry: &HgBlobEntry,
        path: RepoPath,
    ) -> BoxFuture<(), Error> {
        let inner_mutex = self.inner.clone();
        entry
            .get_parents(ctx)
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
    pub fn process_root_manifest(
        &self,
        ctx: CoreContext,
        entry: &HgBlobEntry,
    ) -> BoxFuture<(), Error> {
        if entry.get_type() != manifest::Type::Tree {
            return future::err(
                ErrorKind::NotAManifest(entry.get_hash().into_nodehash(), entry.get_type()).into(),
            )
            .boxify();
        }
        self.process_one_entry(ctx, entry, RepoPath::root())
    }

    pub fn process_one_entry(
        &self,
        ctx: CoreContext,
        entry: &HgBlobEntry,
        path: RepoPath,
    ) -> BoxFuture<(), Error> {
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
                self.process_manifest(ctx, entry, path),
            )
        } else {
            STATS::process_file_entry.add_value(1);
            (
                format!(
                    "While processing file with id {} and path {}",
                    entry.get_hash(),
                    path
                ),
                self.find_parents(ctx, &entry, path),
            )
        };

        fut.chain_err(err_context).from_err().boxify()
    }

    // Check the blobstore to see whether a particular node is present.
    fn assert_in_blobstore(
        ctx: CoreContext,
        blobstore: RepoBlobstore,
        node_id: HgNodeHash,
        is_tree: bool,
    ) -> BoxFuture<(), Error> {
        let key = if is_tree {
            HgManifestId::new(node_id).blobstore_key()
        } else {
            HgFileNodeId::new(node_id).blobstore_key()
        };
        blobstore.assert_present(ctx, key)
    }

    pub fn finalize(
        self,
        ctx: CoreContext,
        filenodes: Arc<dyn Filenodes>,
        cs_id: HgNodeHash,
        mf_id: HgManifestId,
        parent_manifest_ids: Vec<HgManifestId>,
    ) -> BoxFuture<(), Error> {
        let required_checks = {
            let inner = self.inner.lock().expect("Lock poisoned");
            let blobstore = inner.blobstore.clone();
            let boxed_blobstore = blobstore.boxed();
            find_intersection_of_diffs(
                ctx.clone(),
                boxed_blobstore.clone(),
                mf_id,
                parent_manifest_ids,
            )
            .map({
                cloned!(ctx);
                move |(path, entry)| {
                    let (node, is_tree) = match entry {
                        Entry::Tree(mf_id) => (mf_id.into_nodehash(), true),
                        Entry::Leaf((_, file_id)) => (file_id.into_nodehash(), false),
                    };

                    let assert =
                        Self::assert_in_blobstore(ctx.clone(), blobstore.clone(), node, is_tree);

                    assert
                        .with_context(move |_| format!("While checking for path: {:?}", path))
                        .map_err(Error::from)
                }
            })
            .buffer_unordered(100)
            .collect()
            .map(|checks| {
                STATS::finalize_required.add_value(checks.len() as i64);
            })
            .timed({
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
                        ctx.clone(),
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

        let upload_filenodes = {
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

            if inner.draft {
                future::ok(()).left_future()
            } else {
                let filenodeinfos = stream::futures_unordered(uploaded_entries.into_iter().map(
                    |(path, blobentry)| {
                        blobentry
                            .get_envelope(ctx.clone())
                            .and_then(move |envelope| {
                                let parents = envelope.get_parents();
                                let copy_from = compute_copy_from_info(&path, &envelope);

                                copy_from.map(move |copy_from| {
                                    let (p1, p2) = parents.get_nodes();
                                    FilenodeInfo {
                                        path,
                                        filenode: HgFileNodeId::new(
                                            blobentry.get_hash().into_nodehash(),
                                        ),
                                        p1: p1.map(HgFileNodeId::new),
                                        p2: p2.map(HgFileNodeId::new),
                                        copyfrom: copy_from,
                                        linknode: HgChangesetId::new(cs_id),
                                    }
                                })
                            })
                    },
                ))
                .boxify();

                filenodes
                    .add_filenodes(ctx, filenodeinfos, inner.repoid)
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
                    .right_future()
            }
        };

        parent_checks
            .join3(required_checks, upload_filenodes)
            .map(|_| ())
            .boxify()
    }
}

fn compute_copy_from_info(
    path: &RepoPath,
    envelope: &Box<dyn HgBlobEnvelope>,
) -> Result<Option<(RepoPath, HgFileNodeId)>> {
    match path {
        &RepoPath::FilePath(_) => {
            STATS::finalize_compute_copy_from_info.add_value(1);
            envelope
                .get_copy_info()
                .map(|copiedfrom| copiedfrom.map(|(path, node)| (RepoPath::FilePath(path), node)))
        }
        &RepoPath::RootPath | &RepoPath::DirectoryPath(_) => {
            // No copy information for directories/repo roots
            Ok(None)
        }
    }
}

fn compute_changed_files_pair(
    ctx: CoreContext,
    to: &Box<dyn HgManifest + Sync>,
    from: &Box<dyn HgManifest + Sync>,
) -> BoxFuture<HashSet<MPath>, Error> {
    changed_entry_stream(ctx, to, from, None)
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
    ctx: CoreContext,
    repo: BlobRepo,
    root_mf_id: HgManifestId,
    p1_mf_id: Option<&HgManifestId>,
    p2_mf_id: Option<&HgManifestId>,
) -> BoxFuture<Vec<MPath>, Error> {
    let root_mf = repo.get_manifest_by_nodeid(ctx.clone(), root_mf_id);

    let p1_mf = match p1_mf_id {
        Some(p1_mf_id) => repo
            .get_manifest_by_nodeid(ctx.clone(), *p1_mf_id)
            .map(Some)
            .boxify(),
        None => ok(None).boxify(),
    };

    let p2_mf = match p2_mf_id {
        Some(p2_mf_id) => repo
            .get_manifest_by_nodeid(ctx.clone(), *p2_mf_id)
            .map(Some)
            .boxify(),
        None => ok(None).boxify(),
    };

    root_mf
        .join3(p1_mf, p2_mf)
        .and_then(move |(root_mf, p1_mf, p2_mf)| {
            compute_changed_files_impl(ctx, &root_mf, p1_mf.as_ref(), p2_mf.as_ref())
        })
        .boxify()
}

fn compute_changed_files_impl(
    ctx: CoreContext,
    root: &Box<dyn HgManifest + Sync>,
    p1: Option<&Box<dyn HgManifest + Sync>>,
    p2: Option<&Box<dyn HgManifest + Sync>>,
) -> BoxFuture<Vec<MPath>, Error> {
    let empty = manifest::HgEmptyManifest {}.boxed();
    match (p1, p2) {
        (None, None) => compute_changed_files_pair(ctx, &root, &empty),
        (Some(manifest), None) | (None, Some(manifest)) => {
            compute_changed_files_pair(ctx, &root, &manifest)
        }
        (Some(p1), Some(p2)) => {
            let f1 = compute_changed_files_pair(ctx.clone(), &root, &p1)
                .join(compute_changed_files_pair(ctx.clone(), &root, &p2))
                .map(|(left, right)| left.intersection(&right).cloned().collect::<Vec<_>>());

            // Mercurial always includes removed files, we need to match this behaviour
            let f2 = compute_removed_files(ctx.clone(), &root, Some(&p1));
            let f3 = compute_removed_files(ctx.clone(), &root, Some(&p2));

            f1.join3(f2, f3)
                .map(|(ch1, ch2, ch3)| {
                    ch1.into_iter()
                        .chain(ch2.into_iter())
                        .chain(ch3.into_iter())
                        .collect::<HashSet<_>>()
                })
                .boxify()
        }
    }
    .map(|files| {
        let mut files: Vec<MPath> = files.into_iter().collect();
        files.sort_unstable_by(mercurial_mpath_comparator);

        files
    })
    .boxify()
}

fn compute_removed_files(
    ctx: CoreContext,
    child: &Box<dyn HgManifest + Sync>,
    parent: Option<&Box<dyn HgManifest + Sync>>,
) -> impl Future<Item = Vec<MPath>, Error = Error> {
    compute_files_with_status(ctx, child, parent, move |change| match change.status {
        EntryStatus::Deleted(entry) => {
            if entry.get_type() == manifest::Type::Tree {
                None
            } else {
                MPath::join_element_opt(change.dirname.as_ref(), entry.get_name())
            }
        }
        _ => None,
    })
}

fn compute_files_with_status(
    ctx: CoreContext,
    child: &Box<dyn HgManifest + Sync>,
    parent: Option<&Box<dyn HgManifest + Sync>>,
    filter_map: impl Fn(ChangedEntry) -> Option<MPath>,
) -> impl Future<Item = Vec<MPath>, Error = Error> {
    let s = match parent {
        Some(parent) => changed_entry_stream(ctx, child, parent, None).boxify(),
        None => {
            let empty = manifest::HgEmptyManifest {};
            changed_entry_stream(ctx, child, &empty, None).boxify()
        }
    };

    s.filter_map(filter_map).collect()
}

/// Checks if new commit (or to be precise, it's manifest) introduces any new case conflicts
/// It does it in three stages:
/// 1) Checks that there are no case conflicts between added files
/// 2) Checks that added files do not create new case conflicts with already existing files
pub fn check_case_conflicts(
    ctx: CoreContext,
    repo: BlobRepo,
    child_root_mf: HgManifestId,
    parent_root_mf: Option<HgManifestId>,
) -> impl Future<Item = (), Error = Error> {
    let child_mf_fut = repo.get_manifest_by_nodeid(ctx.clone(), child_root_mf.clone());

    let parent_mf_fut = parent_root_mf.map({
        cloned!(ctx, repo);
        move |m| repo.get_manifest_by_nodeid(ctx.clone(), m)
    });

    child_mf_fut
        .join(parent_mf_fut)
        .and_then({
            cloned!(ctx);
            move |(child_mf, parent_mf)| {
                compute_files_with_status(ctx, &child_mf, parent_mf.as_ref(), |change| match change
                    .status
                {
                    EntryStatus::Added(entry) => {
                        if entry.get_type() == manifest::Type::Tree {
                            None
                        } else {
                            MPath::join_element_opt(change.dirname.as_ref(), entry.get_name())
                        }
                    }
                    _ => None,
                })
            }
        })
        .and_then(
            |added_files| match mononoke_types::check_case_conflicts(added_files.clone()) {
                Some(path) => Err(ErrorKind::CaseConflict(path).into()),
                None => Ok(added_files),
            },
        )
        .and_then({
            cloned!(ctx);
            move |added_files| match parent_root_mf {
                Some(parent_root_mf) => {
                    let mut case_conflict_checks = stream::FuturesUnordered::new();
                    for f in added_files {
                        case_conflict_checks.push(
                            repo.check_case_conflict_in_manifest(
                                ctx.clone(),
                                parent_root_mf,
                                child_root_mf,
                                f.clone(),
                            )
                            .map(move |add_conflict| (add_conflict, f)),
                        );
                    }

                    case_conflict_checks
                        .collect()
                        .and_then(|results| {
                            let maybe_conflict =
                                results.into_iter().find(|(add_conflict, _f)| *add_conflict);
                            match maybe_conflict {
                                Some((_, path)) => Err(ErrorKind::CaseConflict(path).into()),
                                None => Ok(()),
                            }
                        })
                        .left_future()
                }
                None => Ok(()).into_future().right_future(),
            }
        })
        .traced(
            ctx.trace(),
            "check_case_conflicts",
            trace_args! {
                "child_manifest_id" => child_root_mf.to_string(),
                "parent_manifest_id" => parent_root_mf
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_string()),
            },
        )
}

fn mercurial_mpath_comparator(a: &MPath, b: &MPath) -> ::std::cmp::Ordering {
    a.to_vec().cmp(&b.to_vec())
}

pub fn process_entries(
    ctx: CoreContext,
    entry_processor: &UploadEntries,
    root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    new_child_entries: BoxStream<(HgBlobEntry, RepoPath), Error>,
) -> BoxFuture<HgManifestId, Error> {
    let root_manifest_fut = root_manifest
        .context("While uploading root manifest")
        .from_err()
        .and_then({
            cloned!(ctx, entry_processor);
            move |root_manifest| match root_manifest {
                None => future::ok(None).boxify(),
                Some((entry, path)) => {
                    let hash = entry.get_hash().into_nodehash();
                    if entry.get_type() == manifest::Type::Tree && path == RepoPath::RootPath {
                        entry_processor
                            .process_root_manifest(ctx, &entry)
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
            cloned!(ctx, entry_processor);
            move |(entry, path)| entry_processor.process_one_entry(ctx.clone(), &entry, path)
        })
        .buffer_unordered(100)
        .for_each(|()| future::ok(()));

    let mut scuba_logger = entry_processor.scuba_logger();
    root_manifest_fut
        .join(child_entries_fut)
        .and_then(move |(root_hash, ())| match root_hash {
            None => future::ok(HgManifestId::new(NULL_HASH)).boxify(),
            Some(root_hash) => future::ok(HgManifestId::new(root_hash)).boxify(),
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
        (Some(p), None) | (None, Some(p)) => p
            .completion_future
            .clone()
            .and_then(|_| future::ok(()).shared())
            .boxify(),
        (Some(p1), Some(p2)) => p1
            .completion_future
            .clone()
            // DO NOT replace and_then() with join() or futures_ordered()!
            // It may result in a combinatoral explosion in mergy repos, like the following:
            //  o
            //  |\
            //  | o
            //  |/|
            //  o |
            //  |\|
            //  | o
            //  |/|
            //  o |
            //  |\|
            //  ...
            //  |/|
            //  | ~
            //  o
            //  |\
            //  ~ ~
            //
            .and_then({
                let p2_completion_future = p2.completion_future.clone();
                move |_| p2_completion_future
            })
            .and_then(|_| future::ok(()).shared())
            .boxify(),
    }
    .boxify()
}

pub fn handle_parents(
    mut scuba_logger: ScubaSampleBuilder,
    p1: Option<ChangesetHandle>,
    p2: Option<ChangesetHandle>,
) -> BoxFuture<(HgParents, Vec<HgManifestId>, Vec<ChangesetId>), Error> {
    let p1 = p1.map(|cs| cs.can_be_parent);
    let p2 = p2.map(|cs| cs.can_be_parent);
    let p1 = match p1 {
        Some(p1) => p1.map(Some).boxify(),
        None => future::ok(None).boxify(),
    };
    let p2 = match p2 {
        Some(p2) => p2.map(Some).boxify(),
        None => future::ok(None).boxify(),
    };

    // DO NOT replace and_then() with join() or futures_ordered()!
    // It may result in a combinatoral explosion in mergy repos, like the following:
    //  o
    //  |\
    //  | o
    //  |/|
    //  o |
    //  |\|
    //  | o
    //  |/|
    //  o |
    //  |\|
    //  ...
    //  |/|
    //  | ~
    //  o
    //  |\
    //  ~ ~
    //
    p1.and_then(|p1| p2.map(|p2| (p1, p2)))
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
                let parents = HgParents::new(p1_hash, p2_hash);
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
