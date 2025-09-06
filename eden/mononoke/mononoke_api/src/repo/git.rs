/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks_cache::BookmarksCacheRef;
use bundle_uri::GitBundleUriRef;
use bytes::Bytes;
use chrono::DateTime;
use chrono::FixedOffset;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use futures::TryStreamExt;
use git_ref_content_mapping::GitRefContentMappingEntry;
use git_ref_content_mapping::GitRefContentMappingRef;
use git_symbolic_refs::GitSymbolicRefsRef;
use git_types::GitError;
use gix_hash::ObjectId;
use hook_manager::BookmarkState;
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime as MononokeDateTime;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTag;
use mononoke_types::hash::GitSha1;
use packfile::bundle::BundleWriter;
use packfile::bundle::RefNaming;
use packfile::pack::DeltaForm;
use protocol::generator::generate_pack_item_stream;
use protocol::types::ChainBreakingMode;
use protocol::types::DeltaInclusion;
use protocol::types::PackItemStreamRequest;
use protocol::types::PackfileItemInclusion;
use protocol::types::RequestedRefs;
use protocol::types::RequestedSymrefs;
use protocol::types::TagInclusion;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::SortedVectorMap;

use crate::MononokeRepo;
use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::repo::RepoBlobstoreArc;
use crate::repo::RepoContext;

const HGGIT_MARKER_EXTRA: &str = "hg-git-rename-source";
const HGGIT_MARKER_VALUE: &[u8] = b"git";
const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";
const GIT_OBJECT_PREFIX: &str = "git_object";
const SEPARATOR: &str = ".";
const BUNDLE_HEAD: &str = "BUNDLE_HEAD";

impl<R: MononokeRepo> RepoContext<R> {
    /// Set the bonsai to git mapping based on the changeset
    /// If the user is trusted, this will use the hggit extra
    /// Otherwise, it will only work if we can derive a git commit ID, and that ID matches the hggit extra
    /// or the hggit extra is missing from the changeset completely.
    pub async fn set_git_mapping_from_changeset(
        &self,
        changeset_ctx: &ChangesetContext<R>,
        hg_extras: &SortedVectorMap<String, Vec<u8>>,
    ) -> Result<(), MononokeError> {
        //TODO(simonfar): Once we support deriving git commits, do derivation here
        // If there's no hggit extras, then give back the derived hash.
        // If there's a hggit extra, and it matches the derived commit, accept even if you
        // don't have permission
        if hg_extras.get(HGGIT_MARKER_EXTRA).map(Vec::as_slice) == Some(HGGIT_MARKER_VALUE) {
            if let Some(hggit_sha1) = hg_extras.get(HGGIT_COMMIT_ID_EXTRA) {
                // We can't derive right now, so always do the permission check for
                // overriding in the case of mismatch.
                self.authorization_context()
                    .require_override_git_mapping(self.ctx(), self.repo())
                    .await?;

                let hggit_sha1 = String::from_utf8_lossy(hggit_sha1).parse()?;
                let entry = BonsaiGitMappingEntry::new(hggit_sha1, changeset_ctx.id());
                let mapping = self.repo().bonsai_git_mapping();
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

    /// Upload serialized git objects. Applies for all git object types except git blobs.
    pub async fn upload_non_blob_git_object(
        &self,
        git_hash: &gix_hash::oid,
        raw_content: Vec<u8>,
    ) -> anyhow::Result<(), GitError> {
        upload_non_blob_git_object(
            &self.ctx,
            self.repo().repo_blobstore(),
            git_hash,
            raw_content,
        )
        .await
    }

    /// Create Mononoke counterpart of Git tree object
    pub async fn create_git_tree(
        &self,
        git_tree_hash: &gix_hash::oid,
    ) -> anyhow::Result<(), GitError> {
        create_git_tree(&self.ctx, self.repo(), git_tree_hash).await
    }

    /// Create a new annotated tag in the repository.
    pub async fn create_annotated_tag(
        &self,
        tag_object_id: Option<ObjectId>,
        name: String,
        author: Option<String>,
        author_date: Option<DateTime<FixedOffset>>,
        annotation: String,
        annotated_tag: BonsaiAnnotatedTag,
        target_is_tag: bool,
    ) -> Result<ChangesetContext<R>, GitError> {
        let new_changeset_id = create_annotated_tag(
            self.ctx(),
            self.repo(),
            tag_object_id,
            name,
            author,
            author_date,
            annotation,
            annotated_tag,
            target_is_tag,
        )
        .await?;

        Ok(ChangesetContext::new(self.clone(), new_changeset_id))
    }

    /// Create a git bundle for the given stack of commits, returning the raw content
    /// of the bundle bytes
    pub async fn repo_stack_git_bundle(
        &self,
        head: ChangesetId,
        base: ChangesetId,
    ) -> Result<Bytes, GitError> {
        repo_stack_git_bundle(self.ctx(), self.repo(), head, base).await
    }

    /// Upload the packfile base item corresponding to the raw git object with the
    /// input git hash
    pub async fn repo_upload_packfile_base_item(
        &self,
        git_hash: &gix_hash::oid,
        raw_content: Vec<u8>,
    ) -> anyhow::Result<(), GitError> {
        upload_packfile_base_item(
            &self.ctx,
            self.repo().repo_blobstore(),
            git_hash,
            raw_content,
        )
        .await
    }
}

/// Free function for uploading serialized git objects. Applies to all
/// git object types except git blobs.
pub async fn upload_non_blob_git_object<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
    raw_content: Vec<u8>,
) -> anyhow::Result<(), GitError>
where
    B: Blobstore + Clone,
{
    git_types::upload_non_blob_git_object(ctx, blobstore, git_hash, raw_content).await
}

/// Free function for uploading packfile item for git base object
pub async fn upload_packfile_base_item<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
    raw_content: Vec<u8>,
) -> anyhow::Result<(), GitError>
where
    B: Blobstore + Clone,
{
    git_types::upload_packfile_base_item(ctx, blobstore, git_hash, raw_content).await?;
    Ok(())
}

/// Free function for creating Mononoke counterpart of Git tree object
pub async fn create_git_tree(
    ctx: &CoreContext,
    repo: &(impl CommitGraphRef + CommitGraphWriterRef + RepoBlobstoreRef + RepoIdentityRef),
    git_tree_hash: &gix_hash::oid,
) -> anyhow::Result<(), GitError> {
    let blobstore_key = format!(
        "{}{}{}",
        GIT_OBJECT_PREFIX,
        SEPARATOR,
        git_tree_hash.to_hex()
    );
    // Before creating the Mononoke version of the git tree, validate if the raw git
    // tree is stored in the blobstore
    let get_result = repo
        .repo_blobstore()
        .get(ctx, &blobstore_key)
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
    changesets_creation::save_changesets(ctx, repo, vec![changeset])
        .await
        .map_err(|e| GitError::StorageFailure(git_tree_hash.to_hex().to_string(), e.into()))
}

/// Free function for generating Git ref content mapping for a given ref name that points to
/// either a blob or tree object, where `git_hash` is the hash of the object that is pointed
/// to by the ref.
pub async fn generate_ref_content_mapping(
    ctx: &CoreContext,
    repo: &impl GitRefContentMappingRef,
    ref_name: String,
    git_hash: ObjectId,
    is_tree: bool,
) -> Result<(), GitError> {
    let git_hash = GitSha1::from_bytes(git_hash.as_bytes())
        .map_err(|_| GitError::InvalidHash(git_hash.to_string()))?;
    repo.git_ref_content_mapping()
        .add_or_update_mappings(
            ctx,
            vec![GitRefContentMappingEntry::new(ref_name, git_hash, is_tree)],
        )
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_string(), e.into()))
}

/// Free function for creating a new annotated tag in the repository.
///
/// Annotated tags are bookmarks of category `Tag` or `Note` which point to one of these
/// annotated tag changesets.
/// Bookmarks of category `Tag` can also represent lightweight tags, pointing directly to
/// a changeset representing a commit.
/// Bookmarks of category `Note` can only represent annotated tags.
/// Bookmarks of category `Branch` are never annotated.
pub async fn create_annotated_tag(
    ctx: &CoreContext,
    repo: &(
         impl CommitGraphRef
         + CommitGraphWriterRef
         + RepoBlobstoreRef
         + BonsaiTagMappingRef
         + RepoIdentityRef
     ),
    tag_hash: Option<ObjectId>,
    name: String,
    author: Option<String>,
    author_date: Option<DateTime<FixedOffset>>,
    annotation: String,
    annotated_tag: BonsaiAnnotatedTag,
    target_is_tag: bool,
) -> Result<mononoke_types::ChangesetId, GitError> {
    let tag_hash = tag_hash.unwrap_or_else(|| ObjectId::null(gix_hash::Kind::Sha1));
    let tag_id = tag_hash.clone();

    // Create the new Bonsai Changeset. The `freeze` method validates
    // that the bonsai changeset is internally consistent.
    let mut changeset = BonsaiChangesetMut {
        message: annotation,
        git_annotated_tag: Some(annotated_tag),
        ..Default::default()
    };
    if let Some(author) = author {
        changeset.author = author;
    }
    if let Some(author_date) = author_date {
        changeset.author_date = MononokeDateTime::new(author_date);
    }

    let changeset = changeset
        .freeze()
        .map_err(|e| GitError::InvalidBonsai(tag_id.to_string(), e.into()))?;

    let changeset_id = changeset.get_changeset_id();
    // Store the created changeset
    changesets_creation::save_changesets(ctx, repo, vec![changeset])
        .await
        .map_err(|e| anyhow::anyhow!("Error in saving changeset {}, Cause: {:?}", changeset_id, e))
        .map_err(|e| GitError::StorageFailure(tag_id.to_string(), e.into()))?;
    let tag_hash = GitSha1::from_bytes(tag_hash.as_bytes())
        .map_err(|_| GitError::InvalidHash(tag_hash.to_string()))?;
    // Create a mapping between the tag name and the metadata changeset
    let mapping_entry = BonsaiTagMappingEntry {
        changeset_id,
        tag_hash,
        tag_name: name,
        target_is_tag,
    };
    repo.bonsai_tag_mapping()
        .add_or_update_mappings(ctx, vec![mapping_entry])
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Error in storing bonsai tag mappings for tag {}, Cause: {:?}",
                tag_id.to_string(),
                e
            )
        })
        .map_err(|e| GitError::StorageFailure(tag_id.to_string(), e.into()))?;
    Ok(changeset_id)
}

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + RepoDerivedDataArc
    + BookmarksRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + GitRefContentMappingRef
    + GitBundleUriRef
    + RepoDerivedDataRef
    + FilestoreConfigRef
    + GitSymbolicRefsRef
    + BookmarksCacheRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + RepoConfigRef
    + Send
    + Sync;

async fn get_git_commit(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
) -> Result<ObjectId, GitError> {
    let maybe_git_sha1 = repo
        .bonsai_git_mapping()
        .get_git_sha1_from_bonsai(ctx, cs_id)
        .await
        .map_err(|e| {
            GitError::PackfileError(format!(
                "Error in fetching Git Sha1 for changeset {:?} through BonsaiGitMapping. Cause: {}",
                cs_id, e
            ))
        })?;
    let git_sha1 = maybe_git_sha1.ok_or_else(|| {
        GitError::PackfileError(format!("Git Sha1 not found for changeset {:?}", cs_id))
    })?;
    ObjectId::from_hex(git_sha1.to_hex().as_bytes()).map_err(|e| {
        GitError::PackfileError(format!(
            "Error in converting GitSha1 {} to GitObjectId. Cause: {}",
            git_sha1.to_hex(),
            e
        ))
    })
}

pub async fn bookmark_exists_with_prefix<'a, 'b>(
    ctx: &CoreContext,
    repo: &'a impl Repo,
    prefix: &'b BookmarkPrefix,
) -> anyhow::Result<bool> {
    let bookmark_with_prefix_count = repo
        .bookmarks()
        .list(
            ctx.clone(),
            bookmarks::Freshness::MaybeStale,
            prefix,
            BookmarkCategory::ALL,
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            1,
        )
        .try_collect::<Vec<_>>()
        .await
        .with_context(|| format!("Error fetching bookmarks with prefix {prefix}"))?
        .len();

    Ok(bookmark_with_prefix_count > 0)
}

pub async fn get_bookmark_state<'a, 'b>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmark: &'b BookmarkKey,
    freshness: bookmarks::Freshness,
) -> anyhow::Result<BookmarkState> {
    let maybe_bookmark_val = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, freshness)
        .await
        .with_context(|| format!("Error fetching bookmark: {}", bookmark))?;
    if let Some(cs_id) = maybe_bookmark_val {
        Ok(BookmarkState::Existing(cs_id))
    } else {
        Ok(BookmarkState::New)
    }
}

/// Free function for creating a Git bundle for the stack of commits
/// ending at `head` with base `base` and returning the bundle contents
pub async fn repo_stack_git_bundle(
    ctx: &CoreContext,
    repo: &impl Repo,
    head: ChangesetId,
    base: ChangesetId,
) -> Result<Bytes, GitError> {
    let requested_refs =
        RequestedRefs::IncludedWithValue([(BUNDLE_HEAD.to_owned(), head)].into_iter().collect());
    // Ensure we don't include the base and any of its ancestors in the bundle
    let already_present = vec![base];
    // NOTE: We are excluding deltas for this bundle since there is a potential for cycles
    // This should not impact perf since the bundle is for a stack of draft commits. Once native
    // git server is rolled out, we can just do git pull at the client side instead of relying on
    // bundles.
    let request = PackItemStreamRequest::new(
        RequestedSymrefs::ExcludeAll, // Need no symrefs for this bundle
        requested_refs,
        already_present,
        DeltaInclusion::Exclude, // We don't need deltas for this bundle
        TagInclusion::AsIs,
        PackfileItemInclusion::Generate,
        ChainBreakingMode::Stochastic,
    );
    let response = generate_pack_item_stream(ctx.clone(), repo, request)
        .await
        .map_err(|e| {
            GitError::PackfileError(format!(
                "Error in generating pack item stream for head {} and base {}. Cause: {}",
                head, base, e
            ))
        })?;
    let base_git_commit = get_git_commit(ctx, repo, base).await?;
    // Ensure that the base commit is included as a prerequisite
    let prereqs = vec![base_git_commit];
    // Convert the included ref into symref since JF expects only symrefs
    let refs_to_include = response
        .included_refs
        .into_iter()
        .map(
            |(ref_name, ref_target)| match ref_name.strip_prefix("refs/") {
                Some(stripped_ref) => (stripped_ref.to_owned(), ref_target.into_object_id()),
                None => (ref_name, ref_target.into_object_id()),
            },
        )
        .collect();

    // Create the bundle writer with the header pre-written
    // A concurrency of 100 is sufficient since the bundle is for a stack of draft commits
    let concurrency = 100;
    let mut writer = BundleWriter::new_with_header(
        Vec::new(),
        refs_to_include,
        prereqs,
        response.num_items as u32,
        concurrency,
        DeltaForm::RefAndOffset,
        RefNaming::AsIs,
    )
    .await
    .map_err(|e| {
        GitError::PackfileError(format!(
            "Error in creating BundleWriter for head {} and base {}. Cause: {}",
            head, base, e
        ))
    })?;
    // Write the packfile item stream to the bundle
    writer.write(response.items).await.map_err(|e| {
        GitError::PackfileError(format!(
            "Error in writing packfile items to bundle for head {} and base {}. Cause: {}",
            head, base, e
        ))
    })?;
    // Finish writing the bundle
    writer.finish().await.map_err(|e| {
        GitError::PackfileError(format!(
            "Error in finishing writing to the bundle for head {} and base {}. Cause: {}",
            head, base, e
        ))
    })?;

    Ok(writer.into_write().into())
}
