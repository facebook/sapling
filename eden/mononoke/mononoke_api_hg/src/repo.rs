/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{self, format_err, Context};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bytes::Bytes;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::{future, stream, Stream, StreamExt, TryStream, TryStreamExt};
use hgproto::GettreepackArgs;
use mercurial_types::blobs::RevlogChangeset;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId};
use metaconfig_types::RepoConfig;
use mononoke_api::{errors::MononokeError, path::MononokePath, repo::RepoContext};
use mononoke_types::{ChangesetId, MPath};
use repo_client::gettreepack_entries;
use segmented_changelog::{CloneData, StreamCloneData, Vertex};

use super::{HgFileContext, HgTreeContext};

#[derive(Clone)]
pub struct HgRepoContext {
    repo: RepoContext,
}

impl HgRepoContext {
    pub(crate) fn new(repo: RepoContext) -> Self {
        Self { repo }
    }

    /// The `CoreContext` for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        &self.repo.ctx()
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// The underlying Mononoke `BlobRepo` backing this repo.
    pub(crate) fn blob_repo(&self) -> &BlobRepo {
        &self.repo().blob_repo()
    }

    /// The configuration for the repository.
    pub(crate) fn config(&self) -> &RepoConfig {
        self.repo.config()
    }

    /// Look up a file in the repo by `HgFileNodeId`.
    pub async fn file(
        &self,
        filenode_id: HgFileNodeId,
    ) -> Result<Option<HgFileContext>, MononokeError> {
        HgFileContext::new_check_exists(self.clone(), filenode_id).await
    }

    /// Look up a tree in the repo by `HgManifestId`.
    pub async fn tree(
        &self,
        manifest_id: HgManifestId,
    ) -> Result<Option<HgTreeContext>, MononokeError> {
        HgTreeContext::new_check_exists(self.clone(), manifest_id).await
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
    pub fn trees_under_path(
        &self,
        path: MononokePath,
        root_versions: impl IntoIterator<Item = HgManifestId>,
        base_versions: impl IntoIterator<Item = HgManifestId>,
        depth: Option<usize>,
    ) -> impl TryStream<Ok = (HgTreeContext, MononokePath), Error = MononokeError> {
        let ctx = self.ctx().clone();
        let blob_repo = self.blob_repo();
        let args = GettreepackArgs {
            rootdir: path.into_mpath(),
            mfnodes: root_versions.into_iter().collect(),
            basemfnodes: base_versions.into_iter().collect(),
            directories: vec![], // Not supported.
            depth,
        };

        gettreepack_entries(ctx, blob_repo, args)
            .compat()
            .map_err(MononokeError::from)
            .and_then({
                let repo = self.clone();
                move |(mfid, path): (HgManifestId, Option<MPath>)| {
                    let repo = repo.clone();
                    async move {
                        let tree = HgTreeContext::new(repo, mfid).await?;
                        let path = MononokePath::new(path);
                        Ok((tree, path))
                    }
                }
            })
    }

    /// This provides the same functionality as
    /// `mononoke_api::RepoContext::location_to_changeset_id`. It just wraps the request and
    /// response using Mercurial specific types.
    pub async fn location_to_hg_changeset_id(
        &self,
        known_descendant: HgChangesetId,
        distance_to_descendant: u64,
        count: u64,
    ) -> Result<Vec<HgChangesetId>, MononokeError> {
        let known_descendent_csid = self
            .blob_repo()
            .get_bonsai_from_hg(self.ctx().clone(), known_descendant)
            .await?
            .ok_or_else(|| {
                MononokeError::InvalidRequest(format!(
                    "hg changeset {} not found",
                    known_descendant
                ))
            })?;
        let result_csids = self
            .repo()
            .location_to_changeset_id(known_descendent_csid, distance_to_descendant, count)
            .await?;
        let hg_id_futures = result_csids.iter().map(|result_csid| {
            self.blob_repo()
                .get_hg_from_bonsai_changeset(self.ctx().clone(), *result_csid)
        });
        future::try_join_all(hg_id_futures)
            .await
            .map_err(MononokeError::from)
    }

    pub async fn revlog_commit_data(
        &self,
        hg_cs_id: HgChangesetId,
    ) -> Result<Option<Bytes>, MononokeError> {
        let ctx = self.ctx();
        let blobstore = self.blob_repo().blobstore();
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

    pub async fn segmented_changelog_clone_data(
        &self,
    ) -> Result<CloneData<HgChangesetId>, MononokeError> {
        const CHUNK_SIZE: usize = 1000;
        let m_clone_data = self.repo().segmented_changelog_clone_data().await?;
        let idmap_list = m_clone_data.idmap.into_iter().collect::<Vec<_>>();
        let mut hg_idmap = HashMap::new();
        for chunk in idmap_list.chunks(CHUNK_SIZE) {
            let csids = chunk.iter().map(|(_, csid)| *csid).collect::<Vec<_>>();
            let mapping = self
                .blob_repo()
                .get_hg_bonsai_mapping(self.ctx().clone(), csids)
                .await
                .context("error fetching hg bonsai mapping")?
                .into_iter()
                .map(|(hgid, csid)| (csid, hgid))
                .collect::<HashMap<_, _>>();
            for (v, csid) in chunk {
                let hgid = mapping.get(&csid).ok_or_else(|| {
                    MononokeError::from(format_err!(
                        "failed to find bonsai '{}' mapping to hg",
                        csid
                    ))
                })?;
                hg_idmap.insert(*v, *hgid);
            }
        }
        let hg_clone_data = CloneData {
            head_id: m_clone_data.head_id,
            flat_segments: m_clone_data.flat_segments,
            idmap: hg_idmap,
        };
        Ok(hg_clone_data)
    }

    pub async fn segmented_changelog_full_idmap_clone_data(
        &self,
    ) -> Result<StreamCloneData<HgChangesetId>, MononokeError> {
        const CHUNK_SIZE: usize = 1000;
        const BUFFERED_BATCHES: usize = 5;
        let m_clone_data = self
            .repo()
            .segmented_changelog_full_idmap_clone_data()
            .await?;
        let hg_idmap_stream = m_clone_data
            .idmap_stream
            .chunks(CHUNK_SIZE)
            .map({
                let blobrepo = self.blob_repo().clone();
                let ctx = self.ctx().clone();
                move |chunk| hg_convert_idmap_chunk(ctx.clone(), blobrepo.clone(), chunk)
            })
            .buffered(BUFFERED_BATCHES)
            .try_flatten()
            .boxed();
        let hg_clone_data = StreamCloneData {
            head_id: m_clone_data.head_id,
            flat_segments: m_clone_data.flat_segments,
            idmap_stream: hg_idmap_stream,
        };
        Ok(hg_clone_data)
    }
}

async fn hg_convert_idmap_chunk(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    chunk: Vec<Result<(Vertex, ChangesetId), anyhow::Error>>,
) -> Result<impl Stream<Item = Result<(Vertex, HgChangesetId), anyhow::Error>>, anyhow::Error> {
    let chunk: Vec<(Vertex, ChangesetId)> = chunk
        .into_iter()
        .collect::<Result<Vec<_>, anyhow::Error>>()?;
    let csids = chunk.iter().map(|(_, csid)| *csid).collect::<Vec<_>>();
    let mapping = blobrepo
        .get_hg_bonsai_mapping(ctx, csids)
        .await
        .context("error fetching hg bonsai mapping")?
        .into_iter()
        .map(|(hgid, csid)| (csid, hgid))
        .collect::<HashMap<_, _>>();
    let converted = chunk.into_iter().map(move |(v, csid)| {
        let hgid = mapping
            .get(&csid)
            .ok_or_else(|| format_err!("failed to find bonsai '{}' mapping to hg", csid))?;
        Ok((v, *hgid))
    });
    Ok(stream::iter(converted))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;
    use std::sync::Arc;

    use anyhow::Error;
    use blobstore::Loadable;
    use fbinit::FacebookInit;
    use mononoke_api::repo::Repo;
    use mononoke_types::ChangesetId;
    use tests_utils::CreateCommitContext;

    use crate::RepoContextHgExt;

    #[fbinit::compat_test]
    async fn test_new_hg_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);

        let blob_repo = blobrepo_factory::new_memblob_empty(None)?;
        let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
        let repo_ctx = RepoContext::new(ctx, Arc::new(repo)).await?;

        let hg = repo_ctx.hg();
        assert_eq!(hg.repo().name(), "test");

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_trees_under_path(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let blob_repo = blobrepo_factory::new_memblob_empty(None)?;

        // Create test stack; child commit modifies 2 directories.
        let commit_1 = CreateCommitContext::new_root(&ctx, &blob_repo)
            .add_file("dir1/a", "1")
            .add_file("dir2/b", "1")
            .add_file("dir3/c", "1")
            .commit()
            .await?;
        let commit_2 = CreateCommitContext::new(&ctx, &blob_repo, vec![commit_1])
            .add_file("dir1/a", "2")
            .add_file("dir3/a/b/c", "1")
            .commit()
            .await?;

        let root_mfid_1 = root_manifest_id(ctx.clone(), &blob_repo, commit_1).await?;
        let root_mfid_2 = root_manifest_id(ctx.clone(), &blob_repo, commit_2).await?;

        let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
        let repo_ctx = RepoContext::new(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        let trees = hg
            .trees_under_path(
                MononokePath::new(None),
                vec![root_mfid_2],
                vec![root_mfid_1],
                Some(2),
            )
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
        blob_repo: &BlobRepo,
        csid: ChangesetId,
    ) -> Result<HgManifestId, Error> {
        let hg_cs_id = blob_repo
            .get_hg_from_bonsai_changeset(ctx.clone(), csid)
            .await?;
        let hg_cs = hg_cs_id.load(&ctx, &blob_repo.get_blobstore()).await?;
        Ok(hg_cs.manifestid())
    }
}
