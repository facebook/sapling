/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use blobstore::Loadable;
use blobstore::LoadableError;
use cloned::cloned;
use either::Either;
use futures::TryStreamExt;
use futures_lazy_shared::LazyShared;
use mononoke_types::content_manifest::ContentManifest;
use mononoke_types::content_manifest::ContentManifestEntry;
use mononoke_types::content_manifest::ContentManifestRollupData;
use mononoke_types::content_manifest::compat;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::fsnode::FsnodeSummary;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use repo_permission_checker::RepoPermissionCheckerRef;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedManifestId;
use restricted_paths::RestrictedPathsArc;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

/// Summary information about the files in a tree.
/// Either a ContentManifestRollupData or an FsnodeSummary.
pub type TreeSummary = Either<ContentManifestRollupData, FsnodeSummary>;

#[derive(Clone)]
pub struct TreeContext<R> {
    repo_ctx: RepoContext<R>,
    id: compat::ContentManifestId,
    manifest: LazyShared<Result<Either<ContentManifest, Fsnode>, MononokeError>>,
}

impl<R: RepoIdentityRef> fmt::Debug for TreeContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "TreeContext(repo_ctx={:?} id={:?})",
            self.repo_ctx().name(),
            self.id()
        )
    }
}

impl<R> TreeContext<R> {
    /// Create a new TreeContext. The tree must exist in the repo and have
    /// had its derived data generated, and the user must be known to have
    /// permission to access the file.
    ///
    /// To construct a `TreeContext` for a tree that might not exist, use
    /// `new_check_exists`.
    pub(crate) fn new_authorized(repo_ctx: RepoContext<R>, id: compat::ContentManifestId) -> Self {
        Self {
            repo_ctx,
            id,
            manifest: LazyShared::new_empty(),
        }
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo_ctx(&self) -> &RepoContext<R> {
        &self.repo_ctx
    }

    pub fn id(&self) -> &compat::ContentManifestId {
        &self.id
    }
}

impl<
    R: RepoBlobstoreRef
        + RestrictedPathsArc
        + RepoPermissionCheckerRef
        + RepoIdentityRef
        + Clone
        + Send
        + Sync
        + 'static,
> TreeContext<R>
{
    /// Create a new TreeContext using an ID that might not exist. Returns
    /// `None` if the tree doesn't exist.
    pub(crate) async fn new_check_exists(
        repo_ctx: RepoContext<R>,
        id: compat::ContentManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        // Access to an arbitrary tree requires full access to the repo,
        // as we do not know which path it corresponds to.
        repo_ctx
            .authorization_context()
            .require_full_repo_read(repo_ctx.ctx(), repo_ctx.repo())
            .await?;

        // Try to load the manifest immediately to see if it exists. Unlike
        // `new_authorized`, if the manifest is missing, we simply return `Ok(None)`.
        match id
            .load(repo_ctx.ctx(), repo_ctx.repo().repo_blobstore())
            .await
        {
            Ok(manifest) => {
                // Log restricted path access if enabled.
                let blake2 = match &id {
                    Either::Left(cm_id) => cm_id.blake2().into_inner(),
                    Either::Right(fsnode_id) => fsnode_id.blake2().into_inner(),
                };
                let manifest_id = RestrictedManifestId::from(&blake2);
                let manifest_type = match &id {
                    Either::Left(_) => ManifestType::ContentManifest,
                    Either::Right(_) => ManifestType::Fsnode,
                };
                restricted_paths::spawn_enforce_restricted_manifest_access(
                    repo_ctx.ctx(),
                    repo_ctx.repo().restricted_paths_arc().clone(),
                    manifest_id,
                    manifest_type,
                    "manifest_new_check_exists",
                    None,
                )
                .await?;

                Ok(Some(Self {
                    repo_ctx,
                    id,
                    manifest: LazyShared::new_ready(Ok(manifest)),
                }))
            }
            Err(LoadableError::Missing(_)) => Ok(None),
            Err(e) => Err(MononokeError::from(Error::from(e))),
        }
    }
}

impl<R: RepoBlobstoreRef + Clone + Send + Sync + 'static> TreeContext<R> {
    async fn manifest(&self) -> Result<Either<ContentManifest, Fsnode>, MononokeError> {
        self.manifest
            .get_or_init(|| {
                cloned!(self.repo_ctx, self.id);
                async move {
                    id.load(repo_ctx.ctx(), repo_ctx.repo().repo_blobstore())
                        .await
                        .map_err(Error::from)
                        .map_err(MononokeError::from)
                }
            })
            .await
    }

    pub async fn summary(&self) -> Result<TreeSummary, MononokeError> {
        let manifest = self.manifest().await?;
        match manifest {
            Either::Left(cm) => Ok(Either::Left(cm.subentries.rollup_data())),
            Either::Right(fsnode) => Ok(Either::Right(fsnode.summary().clone())),
        }
    }

    pub async fn list(
        &self,
    ) -> Result<Vec<(String, Either<ContentManifestEntry, FsnodeEntry>)>, MononokeError> {
        let manifest = self.manifest().await?;
        match manifest {
            Either::Left(cm) => {
                let blobstore = self.repo_ctx.repo().repo_blobstore();
                let ctx = self.repo_ctx.ctx();
                cm.into_subentries(ctx, blobstore)
                    .map_ok(|(elem, entry)| {
                        (
                            String::from_utf8_lossy(elem.as_ref()).to_string(),
                            Either::Left(entry),
                        )
                    })
                    .try_collect()
                    .await
                    .map_err(MononokeError::from)
            }
            Either::Right(fsnode) => Ok(fsnode
                .into_subentries()
                .into_iter()
                .map(|(elem, entry)| {
                    (
                        String::from_utf8_lossy(elem.as_ref()).to_string(),
                        Either::Right(entry),
                    )
                })
                .collect()),
        }
    }
}
