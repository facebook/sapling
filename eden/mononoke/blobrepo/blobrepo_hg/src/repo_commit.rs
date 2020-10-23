/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BlobRepoHg;
use anyhow::{format_err, Context, Error, Result};
use cloned::cloned;
use failure_ext::{Compat, FutureFailureErrorExt, StreamFailureErrorExt};
use futures::{
    future::{FutureExt, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use futures_ext::{
    BoxFuture as OldBoxFuture, BoxStream as OldBoxStream, FutureExt as OldFutureExt,
};
use futures_old::future::{
    self as old_future, result, Future as OldFuture, Shared, SharedError, SharedItem,
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
    HgChangesetId, HgNodeHash, HgNodeKey, HgParents, MPath, RepoPath, NULL_HASH,
};
use mononoke_types::{self, BonsaiChangeset, ChangesetId};

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
            .boxed()
            .compat()
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

/// Checks if new commit (or to be precise, its manifest) introduces any new case conflicts. It
/// does so in three stages:
///
/// - First, if there is no parent, we only check the manifest being added for conflicts.
/// - Second, we build a tree of lower cased paths, then visit the parent's manifest for branches
/// that overlap with this tree, and collect all the paths that do.
/// - Third, we check if there are any case conflicts in the union of the files added by this
/// change and all those paths we found in step 2 (minus those paths that were removed).
///
/// Note that this assumes there are no path conflicts in the parent_root_mf, if any is provided.
/// If there are path conflicts there, this function may report those path conflicts if any file
/// that is touched in one of the directories (case insensitively) with conflicts.
pub async fn check_case_conflicts(
    ctx: &CoreContext,
    repo: &BlobRepo,
    child_root_mf: HgManifestId,
    parent_root_mf: Option<HgManifestId>,
) -> Result<(), Error> {
    let parent_root_mf = match parent_root_mf {
        Some(parent_root_mf) => parent_root_mf,
        None => {
            // We don't have a parent, just check for internal case conflicts here.
            let paths = child_root_mf
                .list_leaf_entries(ctx.clone(), repo.get_blobstore())
                .map_ok(|(path, _)| path)
                .try_collect::<Vec<_>>()
                .await
                .with_context(|| "Error loading manifest")?;

            if let Some(conflict) = mononoke_types::check_case_conflicts(&paths) {
                return Err(ErrorKind::InternalCaseConflict(conflict.0, conflict.1).into());
            }

            return Ok(());
        }
    };

    let mut added = Vec::new();
    let mut deleted = HashSet::new();

    let mut diff = parent_root_mf.diff(ctx.clone(), repo.get_blobstore(), child_root_mf);
    while let Some(diff) = diff
        .try_next()
        .await
        .with_context(|| "Error loading diff")?
    {
        match diff {
            Diff::Added(Some(path), _) => {
                added.push(path);
            }
            Diff::Removed(Some(path), _) => {
                deleted.insert(path);
            }
            _ => {}
        };
    }

    // Check if there any conflicts internal to the change being landed. Past this point, the
    // conflicts we'll report are external (i.e. they are dependent on the parent commit).
    if let Some(conflict) = mononoke_types::check_case_conflicts(added.iter()) {
        return Err(ErrorKind::InternalCaseConflict(conflict.0, conflict.1).into());
    }

    fn lowercase_mpath(e: &MPath) -> Option<Vec<String>> {
        e.into_iter()
            .map(|e| mononoke_types::path::lowercase_mpath_element(e))
            .collect()
    }

    let mut path_tree_builder = PathTreeBuilder::default();

    for path in added.iter() {
        let path = match lowercase_mpath(&path) {
            Some(path) => path,
            None => continue, // We ignore non-UTF8 paths
        };
        path_tree_builder.insert(path);
    }

    let path_tree = Arc::new(path_tree_builder.freeze());

    let candidates = bounded_traversal::bounded_traversal_stream(
        256,
        Some((parent_root_mf, path_tree, None)),
        |(mf_id, path_tree, path)| async move {
            let mf = mf_id.load(ctx.clone(), repo.blobstore()).await?;
            let mut output = vec![];
            let mut recurse = vec![];

            for (name, entry) in mf.list() {
                let lowered_el = match mononoke_types::path::lowercase_mpath_element(&name) {
                    Some(lowered_el) => lowered_el,
                    None => continue,
                };

                if let Some(subtree) = path_tree.as_ref().subentries.get(&lowered_el) {
                    let path = MPath::join_opt_element(path.as_ref(), &name);

                    if let Entry::Tree(sub_mf_id) = entry {
                        recurse.push((sub_mf_id, subtree.clone(), Some(path.clone())));
                    }

                    output.push(path);
                };
            }

            Result::<_, Error>::Ok((output, recurse))
        },
    )
    .map_ok(|entries| stream::iter(entries.into_iter().map(Result::<_, Error>::Ok)))
    .try_flatten()
    .try_collect::<Vec<_>>()
    .await
    .with_context(|| "Error scanning for conflicting paths")?;

    let files = added
        .iter()
        .chain(candidates.iter().filter(|c| !deleted.contains(c)));

    if let Some((child, parent)) = mononoke_types::check_case_conflicts(files) {
        return Err(ErrorKind::ExternalCaseConflict(child, parent).into());
    }

    Ok(())
}

#[derive(Default)]
struct PathTreeBuilder {
    pub subentries: HashMap<String, Self>,
}

impl PathTreeBuilder {
    pub fn insert(&mut self, path: Vec<String>) {
        path.into_iter().fold(self, |node, element| {
            node.subentries
                .entry(element)
                .or_insert_with(Default::default)
        });
    }

    pub fn freeze(self) -> PathTree {
        let subentries = self
            .subentries
            .into_iter()
            .map(|(el, t)| (el, Arc::new(t.freeze())))
            .collect();

        PathTree { subentries }
    }
}

struct PathTree {
    pub subentries: HashMap<String, Arc<Self>>,
}
