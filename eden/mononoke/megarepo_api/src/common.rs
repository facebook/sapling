/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use commit_transformation::MultiMover;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::{TryFutureExt, TryStreamExt};
use manifest::ManifestOps;
use megarepo_error::MegarepoError;
use mononoke_api::{Mononoke, RepoContext};
use mononoke_types::RepositoryId;
use mononoke_types::{
    BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, MPath,
};
use sorted_vector_map::SortedVectorMap;
use std::collections::BTreeMap;
use std::{convert::TryInto, sync::Arc};

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SourceName(pub String);

pub struct SourceAndMovedChangesets {
    pub source: ChangesetId,
    pub moved: BonsaiChangeset,
}

#[async_trait]
pub trait MegarepoOp {
    fn mononoke(&self) -> &Arc<Mononoke>;

    async fn find_repo_by_id(
        &self,
        ctx: &CoreContext,
        repo_id: i64,
    ) -> Result<RepoContext, MegarepoError> {
        let target_repo_id = RepositoryId::new(repo_id.try_into().unwrap());
        let target_repo = self
            .mononoke()
            .repo_by_id_bypass_acl_check(ctx.clone(), target_repo_id)
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| MegarepoError::request(anyhow!("repo not found {}", target_repo_id)))?;
        Ok(target_repo)
    }

    async fn create_single_move_commit(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
        mover: &MultiMover,
        linkfiles: BTreeMap<MPath, Option<FileChange>>,
        source_name: &SourceName,
    ) -> Result<SourceAndMovedChangesets, MegarepoError> {
        let root_fsnode_id = RootFsnodeId::derive(ctx, repo, cs_id)
            .await
            .map_err(Error::from)?;
        let fsnode_id = root_fsnode_id.fsnode_id();
        let entries = fsnode_id
            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
            .try_collect::<Vec<_>>()
            .await?;

        let mut file_changes = vec![];
        for (path, fsnode) in entries {
            let moved = mover(&path)?;

            // Check that path doesn't move to itself - in that case we don't need to
            // delete file
            if moved.iter().find(|cur_path| cur_path == &&path).is_none() {
                file_changes.push((path.clone(), None));
            }

            file_changes.extend(moved.into_iter().map(|target| {
                let fc = FileChange::new(
                    *fsnode.content_id(),
                    *fsnode.file_type(),
                    fsnode.size(),
                    Some((path.clone(), cs_id)),
                );

                (target, Some(fc))
            }));
        }
        file_changes.extend(linkfiles.into_iter());

        // TODO(stash): we need to figure out what parameters to set here
        let moved_bcs = BonsaiChangesetMut {
            parents: vec![cs_id],
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: format!("move commit for source {}", source_name.0),
            extra: SortedVectorMap::new(),
            file_changes: file_changes.into_iter().collect(),
        }
        .freeze()?;

        let source_and_moved_changeset = SourceAndMovedChangesets {
            source: cs_id,
            moved: moved_bcs,
        };
        Ok(source_and_moved_changeset)
    }
}

pub async fn find_bookmark_and_value(
    ctx: &CoreContext,
    repo: &RepoContext,
    bookmark_name: &str,
) -> Result<(BookmarkName, ChangesetId), MegarepoError> {
    let bookmark = BookmarkName::new(bookmark_name.to_string()).map_err(MegarepoError::request)?;

    let cs_id = repo
        .blob_repo()
        .bookmarks()
        .get(ctx.clone(), &bookmark)
        .map_err(MegarepoError::internal)
        .await?
        .ok_or_else(|| MegarepoError::request(anyhow!("bookmark {} not found", bookmark)))?;

    Ok((bookmark, cs_id))
}
