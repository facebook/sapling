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
use futures::TryStreamExt;
use futures_lazy_shared::LazyShared;
use mononoke_types::ContentManifestId;
use mononoke_types::content_manifest::ContentManifest;
use mononoke_types::content_manifest::ContentManifestEntry;
use mononoke_types::content_manifest::ContentManifestRollupData;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use repo_permission_checker::RepoPermissionCheckerRef;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedManifestId;
use restricted_paths::RestrictedPathsArc;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

#[derive(Clone)]
pub struct TreeContext<R> {
    repo_ctx: RepoContext<R>,
    id: ContentManifestId,
    manifest: LazyShared<Result<ContentManifest, MononokeError>>,
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
    pub(crate) fn new_authorized(repo_ctx: RepoContext<R>, id: ContentManifestId) -> Self {
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

    pub fn id(&self) -> &ContentManifestId {
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
        id: ContentManifestId,
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
                let manifest_id = RestrictedManifestId::from(&id.blake2().into_inner());
                restricted_paths::spawn_enforce_restricted_manifest_access(
                    repo_ctx.ctx(),
                    repo_ctx.repo().restricted_paths_arc().clone(),
                    manifest_id,
                    ManifestType::ContentManifest,
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
    async fn manifest(&self) -> Result<ContentManifest, MononokeError> {
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

    pub async fn summary(&self) -> Result<ContentManifestRollupData, MononokeError> {
        Ok(self.manifest().await?.subentries.rollup_data())
    }

    pub async fn list(&self) -> Result<Vec<(String, ContentManifestEntry)>, MononokeError> {
        let manifest = self.manifest().await?;
        let blobstore = self.repo_ctx.repo().repo_blobstore();
        let ctx = self.repo_ctx.ctx();
        manifest
            .into_subentries(ctx, blobstore)
            .map_ok(|(elem, entry)| (String::from_utf8_lossy(elem.as_ref()).to_string(), entry))
            .try_collect()
            .await
            .map_err(MononokeError::from)
    }
}
