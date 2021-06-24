/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore for a repository.

use anyhow::Result;
use mononoke_types::RepositoryId;

use crate::bubble::{Bubble, BubbleId};
use crate::error::EphemeralBlobstoreError;
use crate::store::EphemeralBlobstore;

/// Ephemeral Blobstore for a particular repository.  This is a repo
/// attribute.
#[facet::facet]
pub struct RepoEphemeralBlobstore {
    /// Repository this store is for.
    repo_id: RepositoryId,

    /// Blobstore backing this repo's ephemeral blobstore.
    ephemeral_blobstore: Option<EphemeralBlobstore>,
}

impl RepoEphemeralBlobstore {
    pub fn disabled(repo_id: RepositoryId) -> Self {
        RepoEphemeralBlobstore {
            repo_id,
            ephemeral_blobstore: None,
        }
    }

    pub(crate) fn new(repo_id: RepositoryId, ephemeral_blobstore: EphemeralBlobstore) -> Self {
        RepoEphemeralBlobstore {
            repo_id,
            ephemeral_blobstore: Some(ephemeral_blobstore),
        }
    }

    fn ephemeral_blobstore(&self) -> Result<&EphemeralBlobstore> {
        self.ephemeral_blobstore
            .as_ref()
            .ok_or_else(|| EphemeralBlobstoreError::NoEphemeralBlobstore(self.repo_id).into())
    }

    pub async fn create_bubble(&self) -> Result<Bubble> {
        self.ephemeral_blobstore()?
            .create_bubble(self.repo_id)
            .await
    }

    pub async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble> {
        self.ephemeral_blobstore()?
            .open_bubble(self.repo_id, bubble_id)
            .await
    }
}
