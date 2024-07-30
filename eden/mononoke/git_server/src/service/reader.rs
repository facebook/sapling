/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_git_mapping::BonsaiGitMapping;
use context::CoreContext;
use git_types::fetch_git_object_bytes;
use git_types::GitIdentifier;
use git_types::HeaderState;
use git_types::ObjectContent;
use gix_hash::ObjectId;
use gix_object::ObjectRef;
use import_tools::git_reader::GitReader;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstore;
use rustc_hash::FxHashMap;

use super::uploader::RefMap;

#[derive(Clone)]
pub struct GitObjectStore {
    pub(crate) object_map: FxHashMap<ObjectId, ObjectContent>,
    ctx: CoreContext,
    blobstore: Arc<RepoBlobstore>,
}

impl GitObjectStore {
    pub fn new(
        object_map: FxHashMap<ObjectId, ObjectContent>,
        ctx: &CoreContext,
        blobstore: Arc<RepoBlobstore>,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            object_map,
            blobstore,
        }
    }
}

#[async_trait]
impl GitReader for GitObjectStore {
    async fn get_object(&self, oid: &gix_hash::oid) -> Result<ObjectContent> {
        if let Some(content) = self.object_map.get(oid).cloned() {
            Ok(content)
        } else {
            // The content was not found in our object map. This could be due to the fact that we are referring
            // to objects outside of the pushed packfile (for reasons like looking up parents of commits, etc).
            // In this case, we will try to fetch the object from blobstore
            let git_identifier = GitIdentifier::Basic(GitSha1::from_object_id(oid)?);
            let bytes = fetch_git_object_bytes(
                &self.ctx,
                self.blobstore.clone(),
                &git_identifier,
                HeaderState::Included,
            )
            .await?;
            let parsed = ObjectRef::from_loose(&bytes)
                .context("Failed to convert bytes into git object")?
                .into_owned();
            Ok(ObjectContent { raw: bytes, parsed })
        }
    }
}

/// Struct storing the git to bonsai mappings for the commits received as part of
/// push. For commits outside of the pushed packfile, this struct fetches the mappings
/// from blobstore (if present)
#[derive(Clone)]
pub struct GitMappingsStore {
    ctx: CoreContext,
    stored_mappings: Arc<dyn BonsaiGitMapping>,
    mappings: RefMap,
}

impl GitMappingsStore {
    pub fn new(
        ctx: &CoreContext,
        stored_mappings: Arc<dyn BonsaiGitMapping>,
        mappings: RefMap,
    ) -> Self {
        Self {
            ctx: ctx.clone(),
            stored_mappings,
            mappings,
        }
    }

    pub async fn get_git_sha1(&self, cs_id: &ChangesetId) -> Result<Option<ObjectId>> {
        if let Some(oid) = self.mappings.oid_by_bonsai(cs_id) {
            Ok(Some(oid))
        } else {
            self.stored_mappings
                .get_git_sha1_from_bonsai(&self.ctx, *cs_id)
                .await?
                .map(|oid| oid.to_object_id())
                .transpose()
        }
    }

    pub async fn get_bonsai(&self, oid: &ObjectId) -> Result<Option<ChangesetId>> {
        // The only time we return an empty mapping is when the oid is null. This can happen
        // when the user is trying to create or delete a bookmark.
        if *oid == ObjectId::null(gix_hash::Kind::Sha1) {
            Ok(None)
        } else if let Some(cs_id) = self.mappings.bonsai_by_oid(oid) {
            Ok(Some(cs_id))
        } else {
            let cs_id = self
                .stored_mappings
                .get_bonsai_from_git_sha1(&self.ctx, GitSha1::from_object_id(oid.as_ref())?)
                .await
                .with_context(|| {
                    format!(
                        "Failure in getting bonsai for git sha1 {} during push",
                        oid.to_hex()
                    )
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!("No bonsai found for git sha1 {} during push", oid.to_hex())
                })?;
            Ok(Some(cs_id))
        }
    }
}
