/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use cloned::cloned;
use futures::channel::oneshot;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::TryStreamExt;
use futures::StreamExt;
use futures_ext::future::TryShared;
use futures_ext::FbTryFutureExt;
use futures_stats::TimedTryFutureExt;
use scuba_ext::MononokeScubaSampleBuilder;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use ::manifest::find_intersection_of_diffs;
use ::manifest::Entry;
pub use blobrepo_common::changed_files::compute_changed_files;
use blobstore::Blobstore;
use blobstore::ErrorKind as BlobstoreError;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use context::CoreContext;
use mercurial_types::blobs::fetch_manifest_envelope;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::blobs::HgBlobEnvelope;
use mercurial_types::blobs::HgChangesetContent;
use mercurial_types::nodehash::HgFileNodeId;
use mercurial_types::nodehash::HgManifestId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::NULL_HASH;
use mononoke_types;
use mononoke_types::BlobstoreKey;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;

use crate::errors::*;
use repo_blobstore::RepoBlobstore;

define_stats! {
    prefix = "mononoke.blobrepo_commit";
    process_file_entry: timeseries(Rate, Sum),
    process_tree_entry: timeseries(Rate, Sum),
    parents_checked: timeseries(Rate, Average, Sum),
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
    can_be_parent:
        TryShared<BoxFuture<'static, Result<(ChangesetId, HgNodeHash, HgManifestId), Error>>>,
    // * Shared is required here because a single changeset can have more than one child, and
    //   all of those children will want to refer to the corresponding future for their parents.
    // * The Compat<Error> here is because the error type for Shared (a cloneable wrapper called
    //   SharedError) doesn't implement Fail, and only implements Error if the wrapped type
    //   implements Error.
    completion_future:
        TryShared<BoxFuture<'static, Result<(BonsaiChangeset, HgBlobChangeset), Error>>>,
}

impl ChangesetHandle {
    pub fn new_pending(
        can_be_parent: TryShared<
            BoxFuture<'static, Result<(ChangesetId, HgNodeHash, HgManifestId), Error>>,
        >,
        completion_future: TryShared<
            BoxFuture<'static, Result<(BonsaiChangeset, HgBlobChangeset), Error>>,
        >,
    ) -> Self {
        Self {
            can_be_parent,
            completion_future,
        }
    }

    pub fn ready_cs_handle(
        ctx: CoreContext,
        repo: impl RepoBlobstoreRef + BonsaiHgMappingRef + Clone + Send + Sync + 'static,
        hg_cs: HgChangesetId,
    ) -> Self {
        let (trigger, can_be_parent) = oneshot::channel();
        let can_be_parent = can_be_parent
            .map_err(|e| format_err!("can_be_parent: {:?}", e))
            .boxed()
            .try_shared();

        let bonsai_cs = {
            cloned!(ctx, repo);
            async move {
                let csid = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(&ctx, hg_cs)
                    .await?
                    .ok_or(ErrorKind::BonsaiMappingNotFound(hg_cs))?;
                let bonsai_cs = csid.load(&ctx, repo.repo_blobstore()).await?;
                Ok::<_, Error>(bonsai_cs)
            }
        };

        let completion_future = async move {
            let (bonsai_cs, hg_cs) = future::try_join(
                bonsai_cs,
                hg_cs.load(&ctx, repo.repo_blobstore()).map_err(Error::from),
            )
            .await?;
            let _ = trigger.send((
                bonsai_cs.get_changeset_id(),
                hg_cs.get_changeset_id().into_nodehash(),
                hg_cs.manifestid(),
            ));
            Ok((bonsai_cs, hg_cs))
        }
        .boxed()
        .try_shared();

        Self {
            can_be_parent,
            completion_future,
        }
    }

    pub fn get_completed_changeset(
        self,
    ) -> TryShared<BoxFuture<'static, Result<(BonsaiChangeset, HgBlobChangeset), Error>>> {
        self.completion_future
    }
}

/// State used while tracking uploaded entries, to ensure that a changeset ends up with the right
/// set of blobs uploaded, and all filenodes present.
struct UploadEntriesState {
    /// All the blobs that have been uploaded in this changeset
    uploaded_entries: HashMap<RepoPath, Entry<HgManifestId, HgFileNodeId>>,
    /// Parent hashes (if any) of the blobs that have been uploaded in this changeset. Used for
    /// validation of this upload - all parents must either have been uploaded in this changeset,
    /// or be present in the blobstore before the changeset can complete.
    parents: HashSet<Entry<HgManifestId, HgFileNodeId>>,
}

#[derive(Clone)]
pub struct UploadEntries {
    scuba_logger: MononokeScubaSampleBuilder,
    inner: Arc<Mutex<UploadEntriesState>>,
    blobstore: RepoBlobstore,
}

impl UploadEntries {
    pub fn new(blobstore: RepoBlobstore, scuba_logger: MononokeScubaSampleBuilder) -> Self {
        Self {
            scuba_logger,
            inner: Arc::new(Mutex::new(UploadEntriesState {
                uploaded_entries: HashMap::new(),
                parents: HashSet::new(),
            })),
            blobstore,
        }
    }

    fn scuba_logger(&self) -> MononokeScubaSampleBuilder {
        self.scuba_logger.clone()
    }

    /// The root manifest needs special processing - unlike all other entries, it is required even
    /// if no other manifest references it. Otherwise, this function is the same as
    /// `process_one_entry` and can be called after it.
    /// It is safe to call this multiple times, but not recommended - every manifest passed to
    /// this function is assumed required for this commit, even if it is not the root.
    pub async fn process_root_manifest<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entry: HgManifestId,
    ) -> Result<()> {
        self.process_one_entry(ctx, Entry::Tree(entry), RepoPath::root())
            .await
    }

    pub async fn process_one_entry<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entry: Entry<HgManifestId, HgFileNodeId>,
        path: RepoPath,
    ) -> Result<()> {
        {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            inner.uploaded_entries.insert(path.clone(), entry.clone());
        }

        let parents = match entry {
            Entry::Tree(manifest_id) => {
                STATS::process_tree_entry.add_value(1);

                // NOTE: Just fetch the envelope here, because we don't actually need the
                // deserialized manifest: just the parents will do.
                let envelope = fetch_manifest_envelope(ctx, &self.blobstore, manifest_id)
                    .await
                    .with_context(|| {
                        format!(
                            "Error processing manifest with id {} and path {}",
                            manifest_id, path
                        )
                    })?;

                envelope
                    .get_parents()
                    .into_iter()
                    .map(|p| Entry::Tree(HgManifestId::new(p)))
                    .collect::<Vec<_>>()
            }
            Entry::Leaf(filenode_id) => {
                STATS::process_file_entry.add_value(1);

                let envelope = filenode_id
                    .load(ctx, &self.blobstore)
                    .await
                    .with_context(|| {
                        format!(
                            "Error processing file with id {} and path {}",
                            filenode_id, path
                        )
                    })?;

                envelope
                    .get_parents()
                    .into_iter()
                    .map(|p| Entry::Leaf(HgFileNodeId::new(p)))
                    .collect::<Vec<_>>()
            }
        };

        {
            let mut inner = self.inner.lock().expect("Lock poisoned");
            inner.parents.extend(parents.into_iter());
        }

        Ok(())
    }

    // Check the blobstore to see whether a particular node is present.
    async fn assert_in_blobstore(
        ctx: &CoreContext,
        blobstore: &RepoBlobstore,
        entry: Entry<HgManifestId, HgFileNodeId>,
    ) -> Result<(), Error> {
        match entry {
            Entry::Tree(mfid) => {
                if mfid.into_nodehash() == NULL_HASH {
                    return Ok(());
                }

                let key = mfid.blobstore_key();
                if !blobstore
                    .is_present(ctx, &key)
                    .await?
                    .assume_not_found_if_unsure()
                {
                    return Err(BlobstoreError::NotFound(key).into());
                }
            }
            Entry::Leaf(fnid) => {
                if fnid.into_nodehash() == NULL_HASH {
                    return Ok(());
                }

                let envelope = fnid.load(ctx, &blobstore).await?;

                let key = envelope.content_id().blobstore_key();
                if !blobstore.is_present(ctx, &key).await?.fail_if_unsure()? {
                    return Err(BlobstoreError::NotFound(key).into());
                }
            }
        }

        Ok(())
    }

    pub async fn finalize(
        self,
        ctx: &CoreContext,
        mf_id: HgManifestId,
        parent_manifest_ids: Vec<HgManifestId>,
    ) -> Result<(), Error> {
        // NOTE: we consume self.entries hence the signature, even if we don't actually need
        // mutable access
        let this = &self;

        let required_checks = {
            async move {
                let (stats, ()) = find_intersection_of_diffs(
                    ctx.clone(),
                    this.blobstore.clone().boxed(),
                    mf_id,
                    parent_manifest_ids,
                )
                .try_for_each_concurrent(100, {
                    move |(path, entry)| {
                        async move {
                            let entry = entry.map_leaf(|(_, fnid)| fnid);
                            Self::assert_in_blobstore(ctx, &this.blobstore, entry)
                                .await
                                .with_context(|| format!("Error checking for path: {:?}", path))?;
                            Ok(())
                        }
                        .boxed()
                    }
                })
                .try_timed()
                .await?;

                this.scuba_logger()
                    .add_future_stats(&stats)
                    .log_with_msg("Required checks", None);

                Ok::<_, Error>(())
            }
        };

        let parent_checks = async move {
            let checks: Vec<_> = {
                let inner = this.inner.lock().expect("Lock poisoned");

                inner
                    .parents
                    .iter()
                    .copied()
                    .map(|entry| async move {
                        Self::assert_in_blobstore(ctx, &this.blobstore, entry)
                            .await
                            .with_context(|| {
                                format!("Error checking for a parent node: {:?}", entry)
                            })?;
                        STATS::parents_checked.add_value(1);
                        Result::<_, Error>::Ok(())
                    })
                    .collect()
            };

            STATS::finalize_parent.add_value(checks.len() as i64);

            let (stats, ()) = stream::iter(checks)
                .map(Ok)
                .try_for_each_concurrent(100, |f| f)
                .try_timed()
                .await?;
            this.scuba_logger()
                .add_future_stats(&stats)
                .log_with_msg("Parent checks", None);
            Ok(())
        };

        {
            let mut inner = this.inner.lock().expect("Lock poisoned");
            let uploaded_entries = std::mem::take(&mut inner.uploaded_entries);

            let uploaded_filenodes_cnt = uploaded_entries
                .iter()
                .filter(|&(path, _)| path.is_file())
                .count();
            let uploaded_manifests_cnt = uploaded_entries
                .iter()
                .filter(|&(path, _)| !path.is_file())
                .count();

            STATS::finalize_uploaded.add_value(uploaded_entries.len() as i64);
            STATS::finalize_uploaded_filenodes.add_value(uploaded_filenodes_cnt as i64);
            STATS::finalize_uploaded_manifests.add_value(uploaded_manifests_cnt as i64);

            this.scuba_logger()
                .add("manifests_count", uploaded_manifests_cnt)
                .add("filelogs_count", uploaded_filenodes_cnt)
                .log_with_msg("Size of changeset", None);
        }

        future::try_join(parent_checks, required_checks).await?;
        Ok(())
    }
}

pub async fn process_entries<'a>(
    ctx: &'a CoreContext,
    entry_processor: &'a UploadEntries,
    root_manifest: BoxFuture<'a, Result<Option<(HgManifestId, RepoPath)>>>,
    new_child_entries: BoxStream<'a, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath)>>,
) -> Result<HgManifestId> {
    let root_manifest_fut = async move {
        let root_manifest = root_manifest
            .await
            .context("While uploading root manifest")?;
        match root_manifest {
            None => Ok(None),
            Some((mfid, path)) => {
                if path == RepoPath::RootPath {
                    entry_processor.process_root_manifest(ctx, mfid).await?;
                    Ok(Some(mfid))
                } else {
                    Err(Error::from(ErrorKind::BadRootManifest(mfid)))
                }
            }
        }
    };

    // Not wrapping this future in "async move" causes mismatched opaque types
    // error ¯\_(ツ)_/¯
    let child_entries_fut = async move {
        new_child_entries
            .map_err(|err| err.context("While uploading child entries"))
            .try_for_each_concurrent(100, move |(entry, path)| {
                entry_processor.process_one_entry(ctx, entry, path)
            })
            .await
    };

    let (stats, (root_hash, ())) = future::try_join(root_manifest_fut, child_entries_fut)
        .try_timed()
        .await?;

    entry_processor
        .scuba_logger
        .clone()
        .add_future_stats(&stats)
        .log_with_msg("Upload entries", None);

    match root_hash {
        None => Ok(HgManifestId::new(NULL_HASH)),
        Some(root_hash) => Ok(root_hash),
    }
}

pub fn extract_parents_complete(
    p1: &Option<ChangesetHandle>,
    p2: &Option<ChangesetHandle>,
) -> BoxFuture<'static, Result<(), Error>> {
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
    let p1 = p1.as_ref().map(|p1| p1.completion_future.clone());
    let p2 = p2.as_ref().map(|p2| p2.completion_future.clone());
    async move {
        if let Some(p1) = p1 {
            p1.await?;
        }
        if let Some(p2) = p2 {
            p2.await?;
        }
        Ok::<(), Error>(())
    }
    .boxed()
}

pub async fn handle_parents(
    mut scuba_logger: MononokeScubaSampleBuilder,
    p1: Option<ChangesetHandle>,
    p2: Option<ChangesetHandle>,
) -> Result<(HgParents, Vec<HgManifestId>, Vec<ChangesetId>), Error> {
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
    let (stats, result) = async move {
        let mut bonsai_parents = Vec::new();
        let mut parent_manifest_hashes = Vec::new();
        let p1_hash = match p1 {
            Some(p1) => {
                let (bonsai_cs_id, hash, manifest) = p1.can_be_parent.await?;
                bonsai_parents.push(bonsai_cs_id);
                parent_manifest_hashes.push(manifest);
                Some(hash)
            }
            None => None,
        };
        let p2_hash = match p2 {
            Some(p2) => {
                let (bonsai_cs_id, hash, manifest) = p2.can_be_parent.await?;
                bonsai_parents.push(bonsai_cs_id);
                parent_manifest_hashes.push(manifest);
                Some(hash)
            }
            None => None,
        };
        let parents = HgParents::new(p1_hash, p2_hash);
        Ok::<_, Error>((parents, parent_manifest_hashes, bonsai_parents))
    }
    .try_timed()
    .await?;
    scuba_logger
        .add_future_stats(&stats)
        .log_with_msg("Wait for parents ready", None);
    Ok(result)
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
