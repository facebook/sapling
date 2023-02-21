/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use filestore::hash_bytes;
use filestore::Sha1IncrementalHasher;
use megarepo_error::cloneable_error;
use mononoke_types::hash::GitSha1;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangesetMut;
use thiserror::Error;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::repo::RepoContext;

const HGGIT_MARKER_EXTRA: &str = "hg-git-rename-source";
const HGGIT_MARKER_VALUE: &[u8] = b"git";
const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";

#[derive(Clone, Debug, Error)]
pub enum GitObjectError {
    /// The provided hash and the derived hash do not match for the given content.
    #[error("Input hash {0} does not match the SHA1 hash {1} of the content")]
    HashMismatch(String, String),

    /// A new bubble could not be created.
    #[error("Invalid git object content provided for object ID (hash) {0}. Cause: {1}")]
    InvalidContent(String, GitInternalError),

    /// The requested bubble does not exist.  Either it was never created or has expired.
    #[error(
        "The object corresponding to object ID {0} is a git blob. Cannot upload raw blob content."
    )]
    DisallowedBlobObject(String),

    /// Failed to store the git object in Mononoke store.
    #[error("Failed to store the git object (ID: {0}) in blobstore. Cause: {1}")]
    StorageFailure(String, GitInternalError),
}

cloneable_error!(GitInternalError);

impl RepoContext {
    /// Set the bonsai to git mapping based on the changeset
    /// If the user is trusted, this will use the hggit extra
    /// Otherwise, it will only work if we can derive a git commit ID, and that ID matches the hggit extra
    /// or the hggit extra is missing from the changeset completely.
    pub async fn set_git_mapping_from_changeset(
        &self,
        changeset_ctx: &ChangesetContext,
    ) -> Result<(), MononokeError> {
        let mut extras: HashMap<_, _> = changeset_ctx.extras().await?.into_iter().collect();

        //TODO(simonfar): Once we support deriving git commits, do derivation here
        // If there's no hggit extras, then give back the derived hash.
        // If there's a hggit extra, and it matches the derived commit, accept even if you
        // don't have permission

        if extras.get(HGGIT_MARKER_EXTRA).map(Vec::as_slice) == Some(HGGIT_MARKER_VALUE) {
            if let Some(hggit_sha1) = extras.remove(HGGIT_COMMIT_ID_EXTRA) {
                // We can't derive right now, so always do the permission check for
                // overriding in the case of mismatch.
                self.authorization_context()
                    .require_override_git_mapping(self.ctx(), self.inner_repo())
                    .await?;

                let hggit_sha1 = String::from_utf8_lossy(&hggit_sha1).parse()?;
                let entry = BonsaiGitMappingEntry::new(hggit_sha1, changeset_ctx.id());
                let mapping = self.inner_repo().bonsai_git_mapping();
                mapping
                    .bulk_add(self.ctx(), &[entry])
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to set git mapping from changeset {}",
                            changeset_ctx.id()
                        )
                    })?;
            }
        }
        Ok(())
    }

    /// Upload serialized git objects
    pub async fn upload_git_object(
        &self,
        git_hash: &git_hash::oid,
        serialized_representation: Vec<u8>,
    ) -> anyhow::Result<(), GitObjectError> {
        // Check if the provided Sha1 hash (i.e. ObjectId) of the bytes actually corresponds to the hash of the bytes
        let bytes = bytes::Bytes::from(serialized_representation);
        let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
        if sha1_hash.as_ref() != git_hash.as_bytes() {
            return Err(GitObjectError::HashMismatch(
                git_hash.to_hex().to_string(),
                sha1_hash.to_hex().to_string(),
            ));
        };
        // Check if the bytes actually correspond to a valid Git object
        let blobstore_bytes = BlobstoreBytes::from_bytes(bytes.clone());
        let git_obj = git_object::ObjectRef::from_loose(bytes.as_ref()).map_err(|e| {
            GitObjectError::InvalidContent(
                git_hash.to_hex().to_string(),
                anyhow::anyhow!(e.to_string()).into(),
            )
        })?;
        // Check if the git object is not a raw content blob. Raw content blobs are uploaded directly through
        // LFS. This method supports git commits, trees, tags, notes and similar pointer objects.
        if let git_object::ObjectRef::Blob(_) = git_obj {
            return Err(GitObjectError::DisallowedBlobObject(
                git_hash.to_hex().to_string(),
            ));
        }
        // The bytes are valid, upload to blobstore with the key:
        // git_object_{hex-value-of-hash}
        let blobstore_key = format!("git_object_{}", git_hash.to_hex());
        self.repo_blobstore()
            .put(&self.ctx, blobstore_key, blobstore_bytes)
            .await
            .map_err(|e| GitObjectError::StorageFailure(git_hash.to_hex().to_string(), e.into()))
    }

    /// Create Mononoke counterpart of Git tree object
    pub async fn create_git_tree(&self, git_tree_hash: &git_hash::oid) -> anyhow::Result<()> {
        let mut changeset = BonsaiChangesetMut::default();
        // Get git hash from tree object ID
        let git_hash = GitSha1::from_bytes(git_tree_hash.as_bytes()).with_context(|| {
            format!(
                "Invalid GitSha1 hash {:?} provided for the Git tree",
                git_tree_hash
            )
        })?;
        // Store hash in the changeset
        changeset.git_tree_hash = Some(git_hash);
        // Freeze the changeset to determine if there are any errors
        let changeset = changeset.freeze().map_err(|e| {
            MononokeError::InvalidRequest(format!("Changes create invalid bonsai changeset: {}", e))
        })?;
        // Store the created changeset
        blobrepo::save_bonsai_changesets(
            vec![changeset.clone()],
            self.ctx().clone(),
            self.inner_repo(),
        )
        .await
        .with_context(|| {
            format!(
                "Failure in saving bonsai changeset for git tree {:?}",
                git_tree_hash
            )
        })
    }
}
