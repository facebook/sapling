/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BlobRepoHg;
use anyhow::{format_err, Error, Result};
use cloned::cloned;
use failure_ext::{Compat, FutureFailureErrorExt, StreamFailureErrorExt};
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{FutureExt, TryFutureExt},
    stream::{FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_ext::{
    BoxFuture as OldBoxFuture, BoxStream as OldBoxStream, FutureExt as OldFutureExt,
};
use futures_old::future::{
    self as old_future, loop_fn, result, Future as OldFuture, Loop, Shared, SharedError, SharedItem,
};
use futures_old::stream::Stream as OldStream;
use futures_old::sync::oneshot;
use futures_old::IntoFuture;
use futures_stats::Timed;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use stats::prelude::*;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::{Arc, Mutex};

use ::manifest::{find_intersection_of_diffs, Diff, Entry, Manifest, ManifestOps};
pub use blobrepo_common::changed_files::compute_changed_files;
use blobstore::{Blobstore, ErrorKind as BlobstoreError, Loadable};
use context::CoreContext;
use mercurial_types::{
    blobs::{ChangesetMetadata, HgBlobChangeset, HgBlobEntry, HgChangesetContent},
    manifest,
    nodehash::{HgFileNodeId, HgManifestId},
    HgChangesetId, HgEntry, HgNodeHash, HgNodeKey, HgParents, MPath, RepoPath, NULL_HASH,
};
use mononoke_types::{self, BonsaiChangeset, ChangesetId, FileType};

use crate::errors::*;
use crate::BlobRepo;
use repo_blobstore::RepoBlobstore;

define_stats! {
    prefix = "mononoke.blobrepo_commit";
    process_file_entry: timeseries(Rate, Sum),
    process_tree_entry: timeseries(Rate, Sum),
    finalize_required: timeseries(Rate, Average, Sum),
    finalize_parent: timeseries(Rate, Average, Sum),
    finalize_uploaded: timeseries(Rate, Average, Sum),
    finalize_uploaded_filenodes: timeseries(Rate, Average, Sum),
    finalize_uploaded_manifests: timeseries(Rate, Average, Sum),
    finalize_compute_copy_from_info: timeseries(Rate, Sum),
}

/// A handle to a possibly incomplete HgBlobChangeset. This is used instead of
/// Future<Item = HgBlobChangeset> where we don't want to fully serialize waiting for completion.
/// For example, `create_changeset` takes these as p1/p2 so that it can handle the blobstore side
/// of creating a new changeset before its parent changesets are complete.
/// See `get_completed_changeset()` for the public API you can use to extract the final changeset
#[derive(Clone)]
pub struct ChangesetHandle {
    can_be_parent: Shared<OldBoxFuture<(ChangesetId, HgNodeHash, HgManifestId), Compat<Error>>>,
    // * Shared is required here because a single changeset can have more than one child, and
    //   all of those children will want to refer to the corresponding future for their parents.
    // * The Compat<Error> here is because the error type for Shared (a cloneable wrapper called
    //   SharedError) doesn't implement Fail, and only implements Error if the wrapped type
    //   implements Error.
    completion_future: Shared<OldBoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>>,
}

impl ChangesetHandle {
    pub fn new_pending(
        can_be_parent: Shared<OldBoxFuture<(ChangesetId, HgNodeHash, HgManifestId), Compat<Error>>>,
        completion_future: Shared<OldBoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>>,
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
                move |csid| csid.load(ctx, repo.blobstore()).compat().from_err()
            });

        let (trigger, can_be_parent) = oneshot::channel();

        let can_be_parent = can_be_parent
            .into_future()
            .map_err(|e| format_err!("can_be_parent: {:?}", e))
            .map_err(Compat)
            .boxify()
            .shared();

        let completion_future = bonsai_cs
            .join(hg_cs.load(ctx, repo.blobstore()).compat().from_err())
            .map_err(Compat)
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
    ) -> Shared<OldBoxFuture<(BonsaiChangeset, HgBlobChangeset), Compat<Error>>> {
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
}

#[derive(Clone)]
pub struct UploadEntries {
    scuba_logger: ScubaSampleBuilder,
    inner: Arc<Mutex<UploadEntriesState>>,
}

impl UploadEntries {
    pub fn new(blobstore: RepoBlobstore, scuba_logger: ScubaSampleBuilder) -> Self {
        Self {
            scuba_logger,
            inner: Arc::new(Mutex::new(UploadEntriesState {
                uploaded_entries: HashMap::new(),
                parents: HashSet::new(),
                blobstore,
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
    ) -> OldBoxFuture<(), Error> {
        if entry.get_type() != manifest::Type::Tree {
            old_future::err(
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
    ) -> OldBoxFuture<(), Error> {
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

                old_future::ok(())
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
    ) -> OldBoxFuture<(), Error> {
        if entry.get_type() != manifest::Type::Tree {
            return old_future::err(
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
    ) -> OldBoxFuture<(), Error> {
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

        fut.context(err_context).from_err().boxify()
    }

    // Check the blobstore to see whether a particular node is present.
    fn assert_in_blobstore(
        ctx: CoreContext,
        blobstore: RepoBlobstore,
        node_id: HgNodeHash,
        is_tree: bool,
    ) -> OldBoxFuture<(), Error> {
        if node_id == NULL_HASH {
            return result(Ok(())).boxify();
        }
        let key = if is_tree {
            HgManifestId::new(node_id).blobstore_key()
        } else {
            HgFileNodeId::new(node_id).blobstore_key()
        };

        async move {
            if blobstore.is_present(ctx, key.clone()).await? {
                Ok(())
            } else {
                Err(BlobstoreError::NotFound(key).into())
            }
        }
        .boxed()
        .compat()
        .boxify()
    }

    pub fn finalize(
        self,
        ctx: CoreContext,
        mf_id: HgManifestId,
        parent_manifest_ids: Vec<HgManifestId>,
    ) -> OldBoxFuture<(), Error> {
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
                        .with_context(move || format!("While checking for path: {:?}", path))
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
                        .with_context(move || {
                            format!("While checking for a parent node: {}", node_key)
                        })
                        .from_err()
                })
                .collect();

            STATS::finalize_parent.add_value(checks.len() as i64);

            old_future::join_all(checks).timed({
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

        {
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
        }

        parent_checks.join(required_checks).map(|_| ()).boxify()
    }
}

async fn compute_files_with_status(
    ctx: &CoreContext,
    repo: &BlobRepo,
    child: HgManifestId,
    parent: Option<HgManifestId>,
    filter_map: impl Fn(Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>) -> Option<MPath>,
) -> Result<Vec<MPath>, Error> {
    let s = match parent {
        Some(parent) => parent
            .diff(ctx.clone(), repo.get_blobstore(), child)
            .compat()
            .left_stream(),
        None => child
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .map(|(path, entry)| Diff::Added(path, entry))
            .compat()
            .right_stream(),
    };

    s.try_filter_map(|e| async { Ok(filter_map(e)) })
        .try_collect()
        .await
}

/// Checks if new commit (or to be precise, it's manifest) introduces any new case conflicts
/// It does it in three stages:
/// 1) Checks that there are no case conflicts between added files
/// 2) Checks that added files do not create new case conflicts with already existing files
pub async fn check_case_conflicts(
    ctx: &CoreContext,
    repo: &BlobRepo,
    child_root_mf: HgManifestId,
    parent_root_mf: Option<HgManifestId>,
) -> Result<(), Error> {
    let added_files =
        compute_files_with_status(
            ctx,
            repo,
            child_root_mf,
            parent_root_mf,
            |diff| match diff {
                Diff::Added(path, _entry) => path,
                _ => None,
            },
        )
        .await?;

    if let Some(conflict) = mononoke_types::check_case_conflicts(added_files.iter()) {
        return Err(ErrorKind::InternalCaseConflict(conflict).into());
    }

    let parent_root_mf = match parent_root_mf {
        Some(parent_root_mf) => parent_root_mf,
        None => {
            return Ok(());
        }
    };

    let mut case_conflict_checks = added_files
        .into_iter()
        .map(|path| async move {
            let conflicting = check_case_conflict_in_manifest(
                repo.clone(),
                ctx.clone(),
                parent_root_mf,
                child_root_mf,
                path.clone(),
            )
            .compat()
            .await?;

            Result::<_, Error>::Ok((path, conflicting))
        })
        .collect::<FuturesUnordered<_>>();

    while let Some(element) = case_conflict_checks.next().await {
        let (path, conflicting) = element?;
        if let Some(conflicting) = conflicting {
            return Err(ErrorKind::ExternalCaseConflict(path, conflicting).into());
        }
    }

    Ok(())
}

pub fn process_entries(
    ctx: CoreContext,
    entry_processor: &UploadEntries,
    root_manifest: OldBoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    new_child_entries: OldBoxStream<(HgBlobEntry, RepoPath), Error>,
) -> OldBoxFuture<HgManifestId, Error> {
    let root_manifest_fut = root_manifest
        .context("While uploading root manifest")
        .from_err()
        .and_then({
            cloned!(ctx, entry_processor);
            move |root_manifest| match root_manifest {
                None => old_future::ok(None).boxify(),
                Some((entry, path)) => {
                    let hash = entry.get_hash().into_nodehash();
                    if entry.get_type() == manifest::Type::Tree && path == RepoPath::RootPath {
                        entry_processor
                            .process_root_manifest(ctx, &entry)
                            .map(move |_| Some(hash))
                            .boxify()
                    } else {
                        old_future::err(Error::from(ErrorKind::BadRootManifest(entry.get_type())))
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
        .for_each(|()| old_future::ok(()));

    let mut scuba_logger = entry_processor.scuba_logger();
    root_manifest_fut
        .join(child_entries_fut)
        .and_then(move |(root_hash, ())| {
            match root_hash {
                None => old_future::ok(HgManifestId::new(NULL_HASH)).boxify(),
                Some(root_hash) => old_future::ok(HgManifestId::new(root_hash)).boxify(),
            }
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
) -> OldBoxFuture<SharedItem<()>, SharedError<Compat<Error>>> {
    match (p1.as_ref(), p2.as_ref()) {
        (None, None) => old_future::ok(()).shared().boxify(),
        (Some(p), None) | (None, Some(p)) => p
            .completion_future
            .clone()
            .and_then(|_| old_future::ok(()).shared())
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
            .and_then(|_| old_future::ok(()).shared())
            .boxify(),
    }
    .boxify()
}

pub fn handle_parents(
    mut scuba_logger: ScubaSampleBuilder,
    p1: Option<ChangesetHandle>,
    p2: Option<ChangesetHandle>,
) -> OldBoxFuture<(HgParents, Vec<HgManifestId>, Vec<ChangesetId>), Error> {
    let p1 = p1.map(|cs| cs.can_be_parent);
    let p2 = p2.map(|cs| cs.can_be_parent);
    let p1 = match p1 {
        Some(p1) => p1.map(Some).boxify(),
        None => old_future::ok(None).boxify(),
    };
    let p2 = match p2 {
        Some(p2) => p2.map(Some).boxify(),
        None => old_future::ok(None).boxify(),
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

/// Check if adding a single path to manifest would cause case-conflict
///
/// Implementation traverses manifest and checks if correspoinding path element is present,
/// if path element is not present, it lowercases current path element and checks if it
/// collides with any existing elements inside manifest. if so it also needs to check that
/// child manifest contains this entry, because it might have been removed.
pub fn check_case_conflict_in_manifest(
    repo: BlobRepo,
    ctx: CoreContext,
    parent_mf_id: HgManifestId,
    child_mf_id: HgManifestId,
    path: MPath,
) -> impl OldFuture<Item = Option<MPath>, Error = Error> {
    let child_mf_id = child_mf_id.clone();
    parent_mf_id
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .from_err()
        .and_then(move |mf| {
            loop_fn(
                (None, mf, path.into_iter()),
                move |(cur_path, mf, mut elements): (Option<MPath>, _, _)| {
                    let element = match elements.next() {
                        None => return old_future::ok(Loop::Break(None)).boxify(),
                        Some(element) => element,
                    };

                    match mf.lookup(&element) {
                        Some(entry) => {
                            let cur_path = MPath::join_opt_element(cur_path.as_ref(), &element);
                            match entry {
                                Entry::Leaf(..) => old_future::ok(Loop::Break(None)).boxify(),
                                Entry::Tree(manifest_id) => manifest_id
                                    .load(ctx.clone(), repo.blobstore())
                                    .compat()
                                    .from_err()
                                    .map(move |mf| Loop::Continue((Some(cur_path), mf, elements)))
                                    .boxify(),
                            }
                        }
                        None => {
                            let element_utf8 = String::from_utf8(Vec::from(element.as_ref()));
                            let mut potential_conflicts = vec![];
                            // Find all entries in the manifests that can potentially be a conflict.
                            // Entry can potentially be a conflict if its lowercased version
                            // is the same as lowercased version of the current element

                            for (basename, _) in mf.list() {
                                let path =
                                    MPath::join_element_opt(cur_path.as_ref(), Some(&basename));
                                match (&element_utf8, std::str::from_utf8(basename.as_ref())) {
                                    (Ok(ref element), Ok(ref basename)) => {
                                        if basename.to_lowercase() == element.to_lowercase() {
                                            potential_conflicts.extend(path);
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            // For each potential conflict we need to check if it's present in
                            // child manifest. If it is, then we've got a conflict, otherwise
                            // this has been deleted and it's no longer a conflict.
                            child_mf_id
                                .find_entries(
                                    ctx.clone(),
                                    repo.get_blobstore(),
                                    potential_conflicts,
                                )
                                .collect()
                                .map(|entries| {
                                    // NOTE: We flatten here because we cannot have a conflict
                                    // at the root.
                                    Loop::Break(entries.into_iter().next().and_then(|x| x.0))
                                })
                                .boxify()
                        }
                    }
                },
            )
        })
}
