/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::format_err;
use blobrepo_hg::BlobRepoHg;
use blobrepo_hg::ChangesetHandle;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::Freshness;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use dag_types::Location;
use edenapi_types::AnyId;
use edenapi_types::UploadToken;
use ephemeral_blobstore::Bubble;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use ephemeral_blobstore::StorageLocation;
use filestore::FetchKey;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::Stream;
use futures::StreamExt;
use futures::TryStream;
use futures::TryStreamExt;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::stream;
use futures_util::try_join;
use hgproto::GettreepackArgs;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_mutation::HgMutationEntry;
use mercurial_mutation::HgMutationStoreRef;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileEnvelopeMut;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::blobs::RevlogChangeset;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::blobs::UploadHgTreeEntry;
use metaconfig_types::RepoConfig;
use mononoke_api::MononokeRepo;
use mononoke_api::errors::MononokeError;
use mononoke_api::repo::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::RepoPath;
use mononoke_types::hash::GitSha1;
use mononoke_types::path::MPath;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_client::find_new_draft_commits_and_derive_filenodes_for_public_roots;
use repo_client::gettreepack_entries;
use repo_update_logger::CommitInfo;
use repo_update_logger::log_new_commits;
use slog::debug;
use unbundle::upload_changeset;

use super::HgFileContext;
use super::HgTreeContext;

#[derive(Clone)]
pub struct HgRepoContext<R> {
    repo_ctx: RepoContext<R>,
}

impl<R: MononokeRepo> HgRepoContext<R> {
    pub(crate) fn new(repo_ctx: RepoContext<R>) -> Self {
        Self { repo_ctx }
    }

    /// The `CoreContext` for this query.
    pub fn ctx(&self) -> &CoreContext {
        self.repo_ctx.ctx()
    }

    /// The `RepoContext` for this query.
    pub fn repo_ctx(&self) -> &RepoContext<R> {
        &self.repo_ctx
    }

    /// The underlying Mononoke `Repo` backing this `HgRepoContext`.
    pub fn repo(&self) -> &R {
        self.repo_ctx().repo()
    }

    /// The configuration for the repository.
    pub(crate) fn config(&self) -> &RepoConfig {
        self.repo_ctx.config()
    }

    /// Create bubble and return its id
    pub async fn create_bubble(
        &self,
        custom_duration: Option<Duration>,
        labels: Vec<String>,
    ) -> Result<Bubble, MononokeError> {
        Ok(self
            .repo_ctx()
            .repo_ephemeral_store_arc()
            .create_bubble(self.ctx(), custom_duration, labels)
            .await?)
    }

    pub fn ephemeral_store(&self) -> Arc<RepoEphemeralStore> {
        self.repo_ctx().repo_ephemeral_store_arc()
    }

    /// Load bubble from id
    pub async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble, MononokeError> {
        self.repo_ctx.open_bubble(bubble_id).await
    }

    /// Get blobstore. If bubble id is present, this is the ephemeral blobstore
    pub async fn bubble_blobstore(
        &self,
        bubble_id: Option<BubbleId>,
    ) -> Result<RepoBlobstore, MononokeError> {
        let main_blobstore = self.repo().repo_blobstore().clone();
        Ok(match bubble_id {
            Some(id) => self
                .repo_ctx
                .open_bubble(id)
                .await?
                .wrap_repo_blobstore(main_blobstore),
            None => main_blobstore,
        })
    }

    /// Get changeset id from hg changeset id
    pub async fn get_bonsai_from_hg(
        &self,
        hgid: HgChangesetId,
    ) -> Result<Option<ChangesetId>, MononokeError> {
        Ok(self
            .repo()
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(self.ctx(), hgid)
            .await?)
    }

    /// Fetch file content size, fails if it doesn't exist.
    pub async fn fetch_file_content_size(
        &self,
        content_id: ContentId,
        bubble_id: Option<BubbleId>,
    ) -> Result<u64, MononokeError> {
        Ok(filestore::get_metadata(
            &self.bubble_blobstore(bubble_id).await?,
            self.ctx(),
            &FetchKey::Canonical(content_id),
        )
        .await?
        .ok_or_else(|| {
            MononokeError::InvalidRequest(format!(
                "failed to fetch or rebuild metadata for ContentId('{}'), file content must be prior uploaded",
                content_id
            ))
        })?
        .total_size)
    }

    async fn is_key_present_in_blobstore(
        &self,
        key: &str,
        bubble_id: Option<BubbleId>,
    ) -> Result<bool, MononokeError> {
        async move {
            self.bubble_blobstore(bubble_id)
                .await?
                .is_present(self.ctx(), key)
                .await
                .map(|is_present| {
                    // if we can't resolve the presence (some blobstores failed, some returned None)
                    // we can re-upload the blob
                    is_present.assume_not_found_if_unsure()
                })
        }
        .await
        .map_err(MononokeError::from)
    }

    /// Look up in blobstore by `ContentId`
    pub async fn is_file_present(
        &self,
        hash: impl Into<FetchKey>,
        bubble_id: Option<BubbleId>,
    ) -> Result<bool, MononokeError> {
        self.is_key_present_in_blobstore(&hash.into().blobstore_key(), bubble_id)
            .await
    }

    /// Convert given hash to canonical ContentId
    pub async fn convert_file_to_content_id<H: Into<FetchKey> + Copy + std::fmt::Debug>(
        &self,
        hash: H,
        bubble_id: Option<BubbleId>,
    ) -> Result<Option<ContentId>, MononokeError> {
        match hash
            .into()
            .load(self.ctx(), &self.bubble_blobstore(bubble_id).await?)
            .await
        {
            Ok(cid) => Ok(Some(cid)),
            Err(LoadableError::Missing(_)) => Ok(None),
            Err(LoadableError::Error(err)) => {
                Err(err).with_context(|| format_err!("While fetching ContentId for {:?}", hash))?
            }
        }
    }

    /// Store file into blobstore
    pub async fn store_file(
        &self,
        key: impl Into<FetchKey>,
        size: u64,
        data: impl Stream<Item = Result<Bytes, Error>> + Send,
        bubble_id: Option<BubbleId>,
    ) -> Result<ContentMetadataV2, MononokeError> {
        filestore::store(
            &self.bubble_blobstore(bubble_id).await?,
            *self.repo().filestore_config(),
            self.ctx(),
            &StoreRequest::with_fetch_key(size, key.into()),
            data,
        )
        .await
        .map_err(MononokeError::from)
    }

    /// Download file contents
    pub async fn download_file(
        &self,
        upload_token: UploadToken,
    ) -> Result<Option<impl Stream<Item = Result<Bytes, Error>> + 'static + use<R>>, MononokeError>
    {
        Ok(filestore::fetch(
            self.bubble_blobstore(upload_token.data.bubble_id.map(BubbleId::new))
                .await?,
            self.ctx().clone(),
            &match upload_token.data.id {
                AnyId::AnyFileContentId(file_id) => file_id.into(),
                e => {
                    return Err(MononokeError::from(format_err!(
                        "Id is not of a file: {:?}",
                        e
                    )));
                }
            },
        )
        .await?)
    }

    /// Test whether a Mercurial changeset exists.
    pub async fn hg_changeset_exists(
        &self,
        hg_changeset_id: HgChangesetId,
    ) -> Result<bool, MononokeError> {
        self.repo()
            .hg_changeset_exists(self.ctx().clone(), hg_changeset_id)
            .await
            .map_err(MononokeError::from)
    }

    /// Test whether a changeset exists in a particular storage location.
    pub async fn changeset_exists(
        &self,
        changeset_id: ChangesetId,
        storage_location: StorageLocation,
    ) -> Result<bool, MononokeError> {
        self.repo_ctx
            .changeset_exists(changeset_id, storage_location)
            .await
    }

    /// Look up in blobstore by `HgFileNodeId`
    pub async fn filenode_exists(&self, filenode_id: HgFileNodeId) -> Result<bool, MononokeError> {
        self.is_key_present_in_blobstore(&filenode_id.blobstore_key(), None)
            .await
    }

    /// Look up in blobstore by `HgManifestId`
    pub async fn tree_exists(&self, manifest_id: HgManifestId) -> Result<bool, MononokeError> {
        self.is_key_present_in_blobstore(&manifest_id.blobstore_key(), None)
            .await
    }

    /// Look up a file in the repo by `HgFileNodeId`.
    pub async fn file(
        &self,
        filenode_id: HgFileNodeId,
    ) -> Result<Option<HgFileContext<R>>, MononokeError> {
        HgFileContext::new_check_exists(self.clone(), filenode_id).await
    }

    /// Look up a tree in the repo by `HgManifestId`.
    pub async fn tree(
        &self,
        manifest_id: HgManifestId,
    ) -> Result<Option<HgTreeContext<R>>, MononokeError> {
        HgTreeContext::new_check_exists(self.clone(), manifest_id).await
    }

    /// Store HgFilenode into blobstore
    pub async fn store_hg_filenode(
        &self,
        filenode_id: HgFileNodeId,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        content_id: ContentId,
        content_size: u64,
        metadata: Bytes,
    ) -> Result<(), MononokeError> {
        let envelope = HgFileEnvelopeMut {
            node_id: filenode_id,
            p1,
            p2,
            content_id,
            content_size,
            metadata,
        };

        self.repo()
            .repo_blobstore()
            .put(
                self.ctx(),
                filenode_id.blobstore_key(),
                envelope.freeze().into_blob().into(),
            )
            .await
            .map_err(MononokeError::from)?;
        Ok(())
    }

    /// Store Tree into blobstore
    pub async fn store_tree(
        &self,
        upload_node_id: HgNodeHash,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
        contents: Bytes,
        computed_node_id: Option<HgNodeHash>,
    ) -> Result<(), MononokeError> {
        if computed_node_id.is_some() {
            self.repo_ctx
                .authorization_context()
                .require_mirror_upload_operations(self.ctx(), self.repo())
                .await?;
        }
        let entry = UploadHgTreeEntry {
            upload_node_id: UploadHgNodeHash::Checked(upload_node_id),
            contents,
            p1,
            p2,
            path: RepoPath::RootPath, // only used for logging
            computed_node_id,
        };
        let (_, upload_future) = entry.upload(
            self.ctx().clone(),
            Arc::new(self.repo().repo_blobstore().clone()),
        )?;

        upload_future.await.map_err(MononokeError::from)?;

        Ok(())
    }

    /// Store HgChangeset. The function also generates bonsai changeset and stores all necessary mappings.
    pub async fn store_hg_changesets(
        &self,
        changesets: Vec<(HgChangesetId, RevlogChangeset, Option<BonsaiChangeset>)>,
        mutations: Vec<HgMutationEntry>,
    ) -> Result<Vec<Result<(HgChangesetId, BonsaiChangeset), MononokeError>>, MononokeError> {
        let mut uploaded_changesets: HashMap<HgChangesetId, ChangesetHandle> = HashMap::new();
        let filelogs = HashMap::new();
        let manifests = HashMap::new();
        for (node, revlog_cs, bonsai) in changesets {
            uploaded_changesets = upload_changeset(
                self.ctx().clone(),
                self.repo().clone(),
                self.ctx().scuba().clone(),
                node,
                &revlog_cs,
                uploaded_changesets,
                &filelogs,
                &manifests,
                None, /* maybe_backup_repo_source (unsupported here) */
                bonsai,
            )
            .await
            .map_err(MononokeError::from)?;
        }
        let mut results = Vec::new();
        let mut hg_changesets = HashSet::new();
        let mut commits_to_log = Vec::new();
        for (hg_cs_id, handle) in uploaded_changesets {
            let result = match handle.get_completed_changeset().await {
                Ok((bonsai, _)) => {
                    hg_changesets.insert(hg_cs_id);
                    commits_to_log.push(CommitInfo::new(&bonsai, None));
                    Ok((hg_cs_id, bonsai))
                }
                Err(e) => Err(MononokeError::from(Error::from(e))),
            };
            results.push(result);
        }
        log_new_commits(self.ctx(), self.repo_ctx().repo(), None, commits_to_log).await;

        if !mutations.is_empty() {
            self.repo()
                .hg_mutation_store()
                .add_entries(self.ctx(), hg_changesets, mutations)
                .await
                .map_err(MononokeError::from)?;
        }

        Ok(results)
    }

    pub async fn fetch_mutations(
        &self,
        hg_changesets: HashSet<HgChangesetId>,
    ) -> Result<Vec<HgMutationEntry>, MononokeError> {
        Ok(self
            .repo()
            .hg_mutation_store()
            .all_predecessors(self.ctx(), hg_changesets)
            .await?)
    }

    /// Request all of the tree nodes in the repo under a given path.
    ///
    /// The caller must specify a list of desired versions of the subtree for
    /// this path, specified as a list of manifest IDs of tree nodes
    /// corresponding to different versions of the root node of the subtree.
    ///
    /// The caller may also specify a list of versions of the subtree to
    /// delta against. The server will only return tree nodes that are in
    /// the requested subtrees that are not in the base subtrees.
    ///
    /// Returns a stream of `HgTreeContext`s, each corresponding to a node in
    /// the requested versions of the subtree, along with its associated path.
    ///
    /// This method is equivalent to Mercurial's `gettreepack` wire protocol
    /// command.
    pub fn trees_under_path<
        T: IntoIterator<Item = HgManifestId>,
        U: IntoIterator<Item = HgManifestId>,
    >(
        &self,
        path: MPath,
        root_versions: T,
        base_versions: U,
        depth: Option<usize>,
    ) -> impl TryStream<Ok = (HgTreeContext<R>, MPath), Error = MononokeError> + use<R, T, U> {
        let ctx = self.ctx().clone();
        let repo = self.repo_ctx.repo();
        let args = GettreepackArgs {
            rootdir: path,
            mfnodes: root_versions.into_iter().collect(),
            basemfnodes: base_versions.into_iter().collect(),
            directories: vec![], // Not supported.
            depth,
        };

        gettreepack_entries(ctx, repo, args)
            .compat()
            .map_err(MononokeError::from)
            .and_then({
                let repo = self.clone();
                move |(mfid, path): (HgManifestId, MPath)| {
                    let repo = repo.clone();
                    async move {
                        let tree = HgTreeContext::new(repo, mfid).await?;
                        Ok((tree, path))
                    }
                }
            })
    }

    // TODO(mbthomas): get_hg_from_bonsai -> derive_hg_changeset
    pub async fn get_hg_from_bonsai(
        &self,
        cs_id: ChangesetId,
    ) -> Result<HgChangesetId, MononokeError> {
        Ok(self.repo().derive_hg_changeset(self.ctx(), cs_id).await?)
    }

    /// This provides the same functionality as
    /// `mononoke_api::RepoContext::location_to_changeset_id`. It just wraps the request and
    /// response using Mercurial specific types.
    pub async fn location_to_hg_changeset_id(
        &self,
        location: Location<HgChangesetId>,
        count: u64,
    ) -> Result<Vec<HgChangesetId>, MononokeError> {
        let cs_location = location
            .and_then_descendant(|descendant| async move {
                self.repo()
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(self.ctx(), descendant)
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!(
                            "hg changeset {} not found",
                            location.descendant
                        ))
                    })
            })
            .await?;
        let result_csids = self
            .repo_ctx()
            .location_to_changeset_id(cs_location, count)
            .await?;
        let hg_id_futures = result_csids
            .iter()
            .map(|result_csid| self.repo().derive_hg_changeset(self.ctx(), *result_csid));
        future::try_join_all(hg_id_futures)
            .await
            .map_err(MononokeError::from)
    }

    /// This provides the same functionality as
    /// `mononoke_api::RepoContext::many_changeset_ids_to_locations`. It just translates to
    /// and from Mercurial types.
    pub async fn many_hg_changeset_ids_to_locations(
        &self,
        hg_master_heads: Vec<HgChangesetId>,
        hg_ids: Vec<HgChangesetId>,
    ) -> Result<HashMap<HgChangesetId, Result<Location<HgChangesetId>, MononokeError>>, MononokeError>
    {
        let all_hg_ids: Vec<_> = hg_ids
            .iter()
            .cloned()
            .chain(hg_master_heads.clone().into_iter())
            .collect();
        let hg_to_bonsai: HashMap<HgChangesetId, ChangesetId> = self
            .repo()
            .get_hg_bonsai_mapping(self.ctx().clone(), all_hg_ids)
            .await?
            .into_iter()
            .collect();
        let master_heads = hg_master_heads
            .iter()
            .map(|master_id| {
                hg_to_bonsai.get(master_id).cloned().ok_or_else(|| {
                    MononokeError::InvalidRequest(format!(
                        "failed to find bonsai equivalent for client head {}",
                        master_id
                    ))
                })
            })
            .collect::<Result<Vec<_>, MononokeError>>()?;

        // We should treat hg_ids as being absolutely any hash. It is perfectly valid for the
        // server to have not encountered the hash that it was given to convert. Filter out the
        // hashes that we could not convert to bonsai.
        let cs_ids = hg_ids
            .iter()
            .filter_map(|hg_id| hg_to_bonsai.get(hg_id).cloned())
            .collect::<Vec<ChangesetId>>();

        let cs_to_blocations = self
            .repo_ctx()
            .many_changeset_ids_to_locations(master_heads, cs_ids)
            .await?;

        let bonsai_to_hg: HashMap<ChangesetId, HgChangesetId> = self
            .repo()
            .get_hg_bonsai_mapping(
                self.ctx().clone(),
                cs_to_blocations
                    .iter()
                    .filter_map(|(_, result)| match result {
                        Ok(l) => Some(l.descendant),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            )
            .await?
            .into_iter()
            .map(|(hg_id, cs_id)| (cs_id, hg_id))
            .collect();
        let response = hg_ids
            .into_iter()
            .filter_map(|hg_id| hg_to_bonsai.get(&hg_id).map(|cs_id| (hg_id, cs_id)))
            .filter_map(|(hg_id, cs_id)| {
                cs_to_blocations
                    .get(cs_id)
                    .map(|cs_result| (hg_id, cs_result.clone()))
            })
            .map(|(hg_id, cs_result)| {
                let cs_result = match cs_result {
                    Ok(cs_location) => cs_location.try_map_descendant(|descendant| {
                        bonsai_to_hg.get(&descendant).cloned().ok_or_else(|| {
                            MononokeError::InvalidRequest(format!(
                                "failed to find hg equivalent for bonsai {}",
                                descendant
                            ))
                        })
                    }),
                    Err(e) => Err(e),
                };
                (hg_id, cs_result)
            })
            .collect::<HashMap<HgChangesetId, Result<Location<HgChangesetId>, MononokeError>>>();

        Ok(response)
    }

    pub async fn revlog_commit_data(
        &self,
        hg_cs_id: HgChangesetId,
    ) -> Result<Option<Bytes>, MononokeError> {
        let ctx = self.ctx();
        let blobstore = self.repo().repo_blobstore();
        let revlog_cs = RevlogChangeset::load(ctx, blobstore, hg_cs_id)
            .await
            .map_err(MononokeError::from)?;
        let revlog_cs = match revlog_cs {
            None => return Ok(None),
            Some(x) => x,
        };

        let mut buffer = Vec::new();
        revlog_cs
            .generate_for_hash_verification(&mut buffer)
            .map_err(MononokeError::from)?;
        Ok(Some(buffer.into()))
    }

    /// resolve a bookmark name to an Hg Changeset
    pub async fn resolve_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        freshness: Freshness,
    ) -> Result<Option<HgChangesetId>, MononokeError> {
        match self
            .repo_ctx
            .resolve_bookmark(&BookmarkKey::new(bookmark)?, freshness)
            .await?
        {
            Some(c) => c.hg_id().await,
            None => Ok(None),
        }
    }

    /// resolve a bookmark name to an Hg Changeset
    pub async fn resolve_bookmark_git(
        &self,
        bookmark: impl AsRef<str>,
        freshness: Freshness,
    ) -> Result<Option<GitSha1>, MononokeError> {
        match self
            .repo_ctx
            .resolve_bookmark(&BookmarkKey::new(bookmark)?, freshness)
            .await?
        {
            Some(c) => c.git_sha1().await,
            None => Ok(None),
        }
    }

    /// Return (at most 10) HgChangesetIds in the range described by the low and high parameters.
    pub async fn get_hg_in_range(
        &self,
        low: HgChangesetId,
        high: HgChangesetId,
    ) -> Result<Vec<HgChangesetId>, MononokeError> {
        const LIMIT: usize = 10;
        let bonsai_hg_mapping = self.repo().bonsai_hg_mapping();
        bonsai_hg_mapping
            .get_hg_in_range(self.ctx(), low, high, LIMIT)
            .await
            .map_err(|e| e.into())
    }

    /// Convert a list of hg changesets to a list of bonsai changesets.
    pub(crate) async fn convert_changeset_ids(
        &self,
        changesets: Vec<HgChangesetId>,
    ) -> Result<Vec<ChangesetId>, MononokeError> {
        Ok(self
            .repo()
            .get_hg_bonsai_mapping(self.ctx().clone(), changesets.to_vec())
            .await
            .context("error fetching hg bonsai mapping")?
            .iter()
            .map(|(_, bcs_id)| *bcs_id)
            .collect())
    }

    /// Check if all changesets in the list are public.
    /// This may treat commits that "recently" became public as draft.
    pub async fn is_all_public(&self, changesets: &[HgChangesetId]) -> Result<bool, MononokeError> {
        let len = changesets.len();
        let public_phases = self
            .repo()
            .phases()
            .get_cached_public(
                self.ctx(),
                self.convert_changeset_ids(changesets.to_vec()).await?,
            )
            .await?;
        Ok(len == public_phases.len())
    }

    /// Return a mapping of commits to their parents that are in the segment of
    /// of the commit graph bounded by common and heads.
    ///
    /// This should be used for public fetches only, since for draft commits we need to
    /// make sure filenodes are derived before sending.
    pub async fn get_graph_mapping_stream(
        &self,
        common: Vec<HgChangesetId>,
        heads: Vec<HgChangesetId>,
    ) -> Result<
        impl Stream<Item = Result<(HgChangesetId, Vec<HgChangesetId>), MononokeError>>
        + 'static
        + use<R>,
        MononokeError,
    > {
        let ctx = self.ctx().clone();
        let repo = self.repo().clone();
        let bonsai_common = self.convert_changeset_ids(common).await?;
        let bonsai_heads = self.convert_changeset_ids(heads).await?;
        debug!(ctx.logger(), "Streaming Commit Graph...");
        let commit_graph_stream = self
            .repo_ctx()
            .repo()
            .commit_graph()
            .ancestors_difference_stream(&ctx, bonsai_heads, bonsai_common)
            .await?
            .map_err(MononokeError::from)
            .map_ok(move |bcs_id| {
                let ctx = ctx.clone();
                let repo = repo.clone();
                async move {
                    repo.get_hg_changeset_and_parents_from_bonsai(ctx, bcs_id)
                        .await
                        .map_err(MononokeError::from)
                }
            })
            .try_buffered(100);
        Ok(commit_graph_stream)
    }

    /// Return a mapping of commits to their parents that are in the segment of
    /// of the commit graph bounded by common and heads.
    ///
    /// We need to make sure filenodes are derived before sending for draft commits.
    /// This method also return commit's phases.
    pub async fn get_graph_mapping(
        &self,
        common: Vec<HgChangesetId>,
        heads: Vec<HgChangesetId>,
    ) -> Result<Vec<(HgChangesetId, (Vec<HgChangesetId>, bool))>, MononokeError> {
        let ctx = self.ctx().clone();
        let repo = self.repo();
        let phases = repo.phases();

        let common_set: HashSet<_> = common.iter().cloned().collect();
        let heads_vec: Vec<_> = heads.to_vec();

        debug!(ctx.logger(), "Calculating Commit Graph...");
        let (draft_commits, missing_commits) = try_join!(
            find_new_draft_commits_and_derive_filenodes_for_public_roots(
                &ctx,
                repo,
                &common_set,
                &heads_vec,
                phases
            ),
            {
                let bonsai_common = self.convert_changeset_ids(common).await?;
                let bonsai_heads = self.convert_changeset_ids(heads).await?;
                self.repo_ctx().repo().commit_graph().ancestors_difference(
                    &ctx,
                    bonsai_heads,
                    bonsai_common,
                )
            }
        )?;

        let cs_parent_mapping = stream::iter(missing_commits.clone())
            .map(move |cs_id| async move {
                let parents = repo
                    .commit_graph()
                    .changeset_parents(self.ctx(), cs_id)
                    .await?;
                Ok::<_, Error>((cs_id, parents))
            })
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?;

        let all_cs_ids = cs_parent_mapping
            .clone()
            .into_iter()
            .flat_map(|(_, parents)| parents)
            .chain(missing_commits)
            .collect::<HashSet<_>>();

        let map_chunk_size = 100;

        let bonsai_hg_mapping = stream::iter(all_cs_ids)
            .chunks(map_chunk_size)
            .map(move |chunk| async move {
                let mapping = self
                    .repo()
                    .get_hg_bonsai_mapping(self.ctx().clone(), chunk.to_vec())
                    .await
                    .context("error fetching hg bonsai mapping")?;
                Ok::<_, Error>(mapping)
            })
            .buffered(25)
            .try_collect::<Vec<Vec<(HgChangesetId, ChangesetId)>>>()
            .await?
            .into_iter()
            .flatten()
            .map(|(hgid, csid)| (csid, hgid))
            .collect::<HashMap<_, _>>();

        let get_hg_id_fn = |cs_id| {
            bonsai_hg_mapping
                .get(&cs_id)
                .cloned()
                .with_context(|| format_err!("failed to find bonsai '{}' mapping to hg", cs_id))
        };

        let hg_parent_mapping = cs_parent_mapping
            .into_iter()
            .map(|(cs_id, cs_parents)| {
                let hg_id = get_hg_id_fn(cs_id)?;
                let hg_parents = cs_parents
                    .into_iter()
                    .map(get_hg_id_fn)
                    .collect::<Result<Vec<HgChangesetId>, Error>>()
                    .map_err(MononokeError::from)?;
                let is_draft = draft_commits.contains(&hg_id);
                Ok((hg_id, (hg_parents, is_draft)))
            })
            .collect::<Result<Vec<_>, MononokeError>>()?;

        Ok(hg_parent_mapping)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use anyhow::Error;
    use blobstore::Loadable;
    use fbinit::FacebookInit;
    use mononoke_api::repo::Repo;
    use mononoke_macros::mononoke;
    use mononoke_types::ChangesetId;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::RepoContextHgExt;

    #[mononoke::fbinit_test]
    async fn test_new_hg_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;

        let hg = repo_ctx.hg();
        assert_eq!(hg.repo_ctx().name(), "repo");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_trees_under_path(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;

        // Create test stack; child commit modifies 2 directories.
        let commit_1 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir1/a", "1")
            .add_file("dir2/b", "1")
            .add_file("dir3/c", "1")
            .commit()
            .await?;
        let commit_2 = CreateCommitContext::new(&ctx, &repo, vec![commit_1])
            .add_file("dir1/a", "2")
            .add_file("dir3/a/b/c", "1")
            .commit()
            .await?;

        let root_mfid_1 = root_manifest_id(ctx.clone(), &repo, commit_1).await?;
        let root_mfid_2 = root_manifest_id(ctx.clone(), &repo, commit_2).await?;

        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        let trees = hg
            .trees_under_path(MPath::ROOT, vec![root_mfid_2], vec![root_mfid_1], Some(2))
            .try_collect::<Vec<_>>()
            .await?;

        let paths = trees
            .into_iter()
            .map(|(_, path)| format!("{}", path))
            .collect::<BTreeSet<_>>();
        let expected = vec!["", "dir3", "dir1", "dir3/a"]
            .into_iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();

        assert_eq!(paths, expected);

        Ok(())
    }

    /// Get the HgManifestId of the root tree manifest for the given commit.
    async fn root_manifest_id(
        ctx: CoreContext,
        repo: &Repo,
        csid: ChangesetId,
    ) -> Result<HgManifestId, Error> {
        let hg_cs_id = repo.derive_hg_changeset(&ctx, csid).await?;
        let hg_cs = hg_cs_id.load(&ctx, &repo.repo_blobstore().clone()).await?;
        Ok(hg_cs.manifestid())
    }
}
