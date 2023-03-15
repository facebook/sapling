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
use chrono::DateTime;
use chrono::FixedOffset;
use filestore::hash_bytes;
use filestore::Sha1IncrementalHasher;
use megarepo_error::cloneable_error;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTag;
use mononoke_types::hash::GitSha1;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::DateTime as MononokeDateTime;
use repo_authorization::RepoWriteOperation;
use sorted_vector_map::SortedVectorMap;
use thiserror::Error;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::repo::RepoContext;

const HGGIT_MARKER_EXTRA: &str = "hg-git-rename-source";
const HGGIT_MARKER_VALUE: &[u8] = b"git";
const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";
const GIT_OBJECT_PREFIX: &str = "git_object";
const SEPARATOR: &str = ".";

#[derive(Clone, Debug, Error)]
pub enum GitError {
    /// The provided hash and the derived hash do not match for the given content.
    #[error("Input hash {0} does not match the SHA1 hash {1} of the content")]
    HashMismatch(String, String),

    /// The input hash is not a valid SHA1 hash.
    #[error("Input hash {0} is not a valid SHA1 git hash")]
    InvalidHash(String),

    /// The raw object content provided do not correspond to a valid git object.
    #[error("Invalid git object content provided for object ID {0}. Cause: {1}")]
    InvalidContent(String, GitInternalError),

    /// The requested bubble does not exist.  Either it was never created or has expired.
    #[error(
        "The object corresponding to object ID {0} is a git blob. Cannot upload raw blob content"
    )]
    DisallowedBlobObject(String),

    /// Failed to get or store the git object in Mononoke store.
    #[error("Failed to get or store the git object (ID: {0}) in blobstore. Cause: {1}")]
    StorageFailure(String, GitInternalError),

    /// The git object doesn't exist in the Mononoke store.
    #[error("The object corresponding to object ID {0} does not exist in the data store")]
    NonExistentObject(String),

    /// The provided git object could not be converted to a valid bonsai changeset.
    #[error(
        "Validation failure while persisting git object (ID: {0}) as a bonsai changeset. Cause: {1}"
    )]
    InvalidBonsai(String, GitInternalError),
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
        let mut extras: HashMap<_, _> = changeset_ctx.hg_extras().await?.into_iter().collect();

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
        raw_content: Vec<u8>,
    ) -> anyhow::Result<(), GitError> {
        // Check if the provided Sha1 hash (i.e. ObjectId) of the bytes actually corresponds to the hash of the bytes
        let bytes = bytes::Bytes::from(raw_content);
        let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
        if sha1_hash.as_ref() != git_hash.as_bytes() {
            return Err(GitError::HashMismatch(
                git_hash.to_hex().to_string(),
                sha1_hash.to_hex().to_string(),
            ));
        };
        // Check if the bytes actually correspond to a valid Git object
        let blobstore_bytes = BlobstoreBytes::from_bytes(bytes.clone());
        let git_obj = git_object::ObjectRef::from_loose(bytes.as_ref()).map_err(|e| {
            GitError::InvalidContent(
                git_hash.to_hex().to_string(),
                anyhow::anyhow!(e.to_string()).into(),
            )
        })?;
        // Check if the git object is not a raw content blob. Raw content blobs are uploaded directly through
        // LFS. This method supports git commits, trees, tags, notes and similar pointer objects.
        if let git_object::ObjectRef::Blob(_) = git_obj {
            return Err(GitError::DisallowedBlobObject(
                git_hash.to_hex().to_string(),
            ));
        }
        // The bytes are valid, upload to blobstore with the key:
        // git_object_{hex-value-of-hash}
        let blobstore_key = format!("{}{}{}", GIT_OBJECT_PREFIX, SEPARATOR, git_hash.to_hex());
        self.repo_blobstore()
            .put(&self.ctx, blobstore_key, blobstore_bytes)
            .await
            .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))
    }

    /// Create Mononoke counterpart of Git tree object
    pub async fn create_git_tree(
        &self,
        git_tree_hash: &git_hash::oid,
    ) -> anyhow::Result<(), GitError> {
        let blobstore_key = format!(
            "{}{}{}",
            GIT_OBJECT_PREFIX,
            SEPARATOR,
            git_tree_hash.to_hex()
        );
        // Before creating the Mononoke version of the git tree, validate if the raw git
        // tree is stored in the blobstore
        let get_result = self
            .repo_blobstore()
            .get(&self.ctx, &blobstore_key)
            .await
            .map_err(|e| GitError::StorageFailure(git_tree_hash.to_hex().to_string(), e.into()))?;
        if get_result.is_none() {
            return Err(GitError::NonExistentObject(
                git_tree_hash.to_hex().to_string(),
            ));
        }
        let mut changeset = BonsaiChangesetMut::default();
        // Get git hash from tree object ID
        let git_hash = GitSha1::from_bytes(git_tree_hash.as_bytes())
            .map_err(|_| GitError::InvalidHash(git_tree_hash.to_hex().to_string()))?;
        // Store hash in the changeset
        changeset.git_tree_hash = Some(git_hash);
        // Freeze the changeset to determine if there are any errors
        let changeset = changeset
            .freeze()
            .map_err(|e| GitError::InvalidBonsai(git_tree_hash.to_hex().to_string(), e.into()))?;

        // Store the created changeset
        changesets_creation::save_changesets(self.ctx(), self.inner_repo(), vec![changeset])
            .await
            .map_err(|e| GitError::StorageFailure(git_tree_hash.to_hex().to_string(), e.into()))
    }

    /// Create a new annotated tag in the repository.
    ///
    /// Annotated tags are bookmarks of category `Tag` or `Note` which point to one of these
    /// annotated tag changesets.
    /// Bookmarks of category `Tag` can also represent lightweight tags, pointing directly to
    /// a changeset representing a commit.
    /// Bookmarks of category `Note` can only represent annotated tags.
    /// Bookmarks of category `Branch` are never annotated.
    ///
    /// TODO: Consider also taking an `Option<Bubble>` in the future to support snapshoting for
    /// tags.
    ///
    /// For git repos with tags, permit_commits_without_parents must be set to True
    pub async fn create_annotated_tag_changeset(
        &self,
        author: String,
        author_date: DateTime<FixedOffset>,
        annotation: String,
        annotated_tag_target: BonsaiAnnotatedTag,
    ) -> Result<ChangesetContext, MononokeError> {
        self.start_write()?;
        self.authorization_context()
            .require_repo_write(
                self.ctx(),
                self.inner_repo(),
                RepoWriteOperation::CreateChangeset,
            )
            .await?;

        let allowed_no_parents = self
            .config()
            .source_control_service
            .permit_commits_without_parents;
        if !allowed_no_parents {
            return Err(MononokeError::InvalidRequest(String::from(
                "Changesets with no parents cannot be created",
            )));
        }

        let author_date = MononokeDateTime::new(author_date);

        // Create the new Bonsai Changeset. The `freeze` method validates
        // that the bonsai changeset is internally consistent.
        let new_changeset = BonsaiChangesetMut {
            parents: Vec::new(),
            author,
            author_date,
            committer: None,
            committer_date: None,
            message: annotation,
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            git_tree_hash: None,
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_annotated_tag: Some(annotated_tag_target),
        }
        .freeze()
        .map_err(|e| {
            MononokeError::InvalidRequest(format!("Changes create invalid bonsai changeset: {}", e))
        })?;

        let new_changeset_id = new_changeset.get_changeset_id();

        self.save_changesets(vec![new_changeset], self.inner_repo(), None)
            .await?;

        Ok(ChangesetContext::new(self.clone(), new_changeset_id))
    }
}
