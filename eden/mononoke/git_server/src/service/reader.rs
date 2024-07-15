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
use context::CoreContext;
use git_types::fetch_git_object_bytes;
use git_types::GitIdentifier;
use git_types::HeaderState;
use git_types::ObjectContent;
use gix_hash::ObjectId;
use gix_object::ObjectRef;
use import_tools::git_reader::GitReader;
use mononoke_types::hash::GitSha1;
use repo_blobstore::RepoBlobstore;
use rustc_hash::FxHashMap;

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
