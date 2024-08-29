/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::sync::Arc;

use blobstore::impl_loadable_storable;
use blobstore::Blobstore;
use bytes::Bytes;
use context::CoreContext;
use filestore::fetch_with_size;
use filestore::hash_bytes;
use filestore::Sha1IncrementalHasher;
use futures::TryStreamExt;
use gix_object::WriteTo;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::BlobstoreBytes;
use packfile::types::BaseObject;
use packfile::types::GitPackfileBaseItem;

use crate::errors::GitError;
use crate::thrift::Tree as ThriftTree;
use crate::thrift::TreeHandle as ThriftTreeHandle;
use crate::Tree;
use crate::TreeHandle;

impl_loadable_storable! {
    handle_type => TreeHandle,
    handle_thrift_type => ThriftTreeHandle,
    value_type => Tree,
    value_thrift_type => ThriftTree,
}

const GIT_OBJECT_PREFIX: &str = "git_object";
const GIT_PACKFILE_BASE_ITEM_PREFIX: &str = "git_packfile_base_item";
const SEPARATOR: &str = ".";

/// Free function for uploading serialized git objects to blobstore.
/// Supports all git object types except blobs.
pub async fn upload_non_blob_git_object<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
    raw_content: Vec<u8>,
) -> anyhow::Result<(), GitError>
where
    B: Blobstore + Clone,
{
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
    let git_obj = gix_object::ObjectRef::from_loose(bytes.as_ref()).map_err(|e| {
        GitError::InvalidContent(
            git_hash.to_hex().to_string(),
            anyhow::anyhow!(e.to_string()).into(),
        )
    })?;
    // Check if the git object is not a raw content blob. Raw content blobs are uploaded directly through
    // LFS. This method supports git commits, trees, tags, notes and similar pointer objects.
    if let gix_object::ObjectRef::Blob(_) = git_obj {
        return Err(GitError::DisallowedBlobObject(
            git_hash.to_hex().to_string(),
        ));
    }
    // The bytes are valid, upload to blobstore with the key:
    // git_object_{hex-value-of-hash}
    let blobstore_key = format!("{}{}{}", GIT_OBJECT_PREFIX, SEPARATOR, git_hash.to_hex());
    blobstore
        .put(ctx, blobstore_key, blobstore_bytes)
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))
    // TODO(rajshar): Create and upload PackfileItem corresponding to the stored git object
}

/// Free function for fetching the raw bytes of stored git objects.
/// Applies to all git object types except blobs.
pub async fn fetch_non_blob_git_object_bytes<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
) -> anyhow::Result<Bytes, GitError>
where
    B: Blobstore + Clone,
{
    let blobstore_key = format!("{}{}{}", GIT_OBJECT_PREFIX, SEPARATOR, git_hash.to_hex());
    let object_bytes = blobstore
        .get(ctx, &blobstore_key)
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))?
        .ok_or_else(|| GitError::NonExistentObject(git_hash.to_hex().to_string()))?;
    Ok(object_bytes.into_raw_bytes())
}

/// Free function for fetching stored git objects. Applies to all git
/// objects except blobs.
pub async fn fetch_non_blob_git_object<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
) -> anyhow::Result<gix_object::Object, GitError>
where
    B: Blobstore + Clone,
{
    // In git, empty tree is a special object: it's present in every git repo and not persisted in
    // the storage.
    if git_hash == gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1) {
        return Ok(gix_object::Object::Tree(gix_object::Tree::empty()));
    }
    let raw_bytes = fetch_non_blob_git_object_bytes(ctx, blobstore, git_hash).await?;
    let object = gix_object::ObjectRef::from_loose(raw_bytes.as_ref()).map_err(|e| {
        GitError::InvalidContent(
            git_hash.to_hex().to_string(),
            anyhow::anyhow!(e.to_string()).into(),
        )
    })?;
    Ok(object.into())
}

/// Enum determining the state of the git header in the raw
/// git object bytes
#[derive(Clone, Debug)]
pub enum HeaderState {
    /// Include the null-terminated git header when fetching the bytes
    /// of the raw git object
    Included,
    /// Do not include the null-terminated git header when fetching the bytes
    /// of the raw git object
    Excluded,
}

/// Enum determining the type of git object identifier
#[derive(Clone, Debug)]
pub enum GitIdentifier {
    Rich(RichGitSha1),
    Basic(GitSha1),
}

impl GitIdentifier {
    fn basic_sha(&self) -> GitSha1 {
        match self {
            GitIdentifier::Rich(rich_sha) => rich_sha.sha1(),
            GitIdentifier::Basic(basic_sha) => basic_sha.clone(),
        }
    }

    fn blob_identifier(&self) -> bool {
        match self {
            GitIdentifier::Rich(rich_sha) => rich_sha.is_blob(),
            GitIdentifier::Basic(_) => true, // May or may not be a blob, we can't know
        }
    }
}

async fn maybe_fetch_blob_bytes(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    sha: &GitSha1,
    header_state: HeaderState,
) -> anyhow::Result<Option<Bytes>> {
    let fetch_key = sha.clone().into();
    let output = fetch_with_size(blobstore, ctx, &fetch_key)
        .await
        .map_err(|e| GitError::StorageFailure(sha.to_hex().to_string(), e.into()))?;
    let (bytes_stream, num_bytes) = match output {
        Some((bytes, num_bytes)) => (bytes, num_bytes),
        None => return Ok(None),
    };
    // The blob object stored in the blobstore exists without the git header. If requested, prepend the git blob header before returning the bytes
    let mut header_bytes = match header_state {
        HeaderState::Included => format!("blob {}\0", num_bytes).into_bytes(),
        HeaderState::Excluded => vec![],
    };
    // We know the number of bytes we are going to write so reserve the buffer to avoid resizing
    header_bytes.reserve(num_bytes as usize);
    bytes_stream
        .try_fold(header_bytes, |mut acc, bytes| async move {
            acc.append(&mut bytes.to_vec());
            anyhow::Ok(acc)
        })
        .await
        .map(|bytes| Some(Bytes::from(bytes)))
}

/// Fetch the raw bytes of stored git objects. Applies to all git objects.
/// Depending on the header_state, the returned bytes might or might not
/// contain the git header for the object.
pub async fn fetch_git_object_bytes(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    identifier: &GitIdentifier,
    header_state: HeaderState,
) -> anyhow::Result<Bytes> {
    let sha = identifier.basic_sha();
    if identifier.blob_identifier() {
        if let Some(blob_bytes) =
            maybe_fetch_blob_bytes(ctx, blobstore.clone(), &sha, header_state.clone()).await?
        {
            return Ok(blob_bytes);
        }
    }
    let git_objectid = sha.to_object_id()?;
    let object = fetch_non_blob_git_object(ctx, &blobstore, git_objectid.as_ref()).await?;
    let mut object_bytes = match header_state {
        HeaderState::Included => object.loose_header().into_vec(),
        HeaderState::Excluded => vec![],
    };
    object.write_to(object_bytes.by_ref())?;
    Ok(Bytes::from(object_bytes))
}

/// Free function for fetching stored git objects. Applies to all git
/// objects.
pub async fn fetch_git_object(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    identifier: &GitIdentifier,
) -> anyhow::Result<gix_object::Object> {
    let raw_bytes =
        fetch_git_object_bytes(ctx, blobstore, identifier, HeaderState::Included).await?;
    let object = gix_object::ObjectRef::from_loose(raw_bytes.as_ref()).map_err(|e| {
        GitError::InvalidContent(
            identifier.basic_sha().to_hex().to_string(),
            anyhow::anyhow!(e.to_string()).into(),
        )
    })?;
    Ok(object.into())
}

/// Free function for uploading packfile item for git base object and
/// returning the uploaded object
pub async fn upload_packfile_base_item<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
    raw_content: Vec<u8>,
) -> anyhow::Result<GitPackfileBaseItem, GitError>
where
    B: Blobstore + Clone,
{
    // Check if packfile base item can be constructed from the provided bytes
    let packfile_base_item = BaseObject::new(Bytes::from(raw_content))
        .and_then(GitPackfileBaseItem::try_from)
        .map_err(|e| {
            GitError::InvalidContent(
                git_hash.to_hex().to_string(),
                anyhow::anyhow!(e.to_string()).into(),
            )
        })?;

    // The bytes are valid, upload to blobstore with the key:
    // git_packfile_base_item_{hex-value-of-hash}
    let blobstore_key = format!(
        "{}{}{}",
        GIT_PACKFILE_BASE_ITEM_PREFIX,
        SEPARATOR,
        git_hash.to_hex()
    );
    blobstore
        .put(
            ctx,
            blobstore_key,
            packfile_base_item.clone().into_blobstore_bytes(),
        )
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))?;
    Ok(packfile_base_item)
}

/// Free function for fetching stored packfile item for base git if it exists.
/// If the object doesn't exist, None is returned instead of an error.
pub async fn fetch_packfile_base_item_if_exists<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
) -> anyhow::Result<Option<GitPackfileBaseItem>, GitError>
where
    B: Blobstore + Clone,
{
    let blobstore_key = format!(
        "{}{}{}",
        GIT_PACKFILE_BASE_ITEM_PREFIX,
        SEPARATOR,
        git_hash.to_hex()
    );
    blobstore
        .get(ctx, &blobstore_key)
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))?
        .map(|obj| GitPackfileBaseItem::from_encoded_bytes(obj.into_raw_bytes()))
        .transpose()
        .map_err(|_| GitError::InvalidPackfileItem(git_hash.to_hex().to_string()))
}

/// Free function for fetching stored packfile item for base git object
pub async fn fetch_packfile_base_item<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
) -> anyhow::Result<GitPackfileBaseItem, GitError>
where
    B: Blobstore + Clone,
{
    fetch_packfile_base_item_if_exists(ctx, blobstore, git_hash)
        .await?
        .ok_or_else(|| GitError::NonExistentObject(git_hash.to_hex().to_string()))
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use bytes::Bytes;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::TestRepoFixture;
    use gix_hash::ObjectId;
    use gix_object::Object;
    use gix_object::Tag;
    use mononoke_macros::mononoke;
    use packfile::types::to_vec_bytes;
    use packfile::types::BaseObject;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreArc;
    use repo_derived_data::RepoDerivedData;
    use repo_identity::RepoIdentity;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    struct TestRepo(
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        RepoBlobstore,
        RepoDerivedData,
        RepoIdentity,
        CommitGraph,
        dyn CommitGraphWriter,
        FilestoreConfig,
    );

    #[mononoke::fbinit_test]
    async fn store_and_load_packfile_base_item(fb: FacebookInit) -> Result<()> {
        let repo: TestRepo = fixtures::Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let blobstore = repo.repo_blobstore_arc();
        // Create a random Git object and get its bytes
        let tag_bytes = Bytes::from(to_vec_bytes(&Object::Tag(Tag {
            target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            target_kind: gix_object::Kind::Tree,
            name: "TreeTag".into(),
            tagger: None,
            message: "Tag pointing to a tree".into(),
            pgp_signature: None,
        }))?);
        // Create the base object using the Git bytes. This will be used later for comparision
        let base_object = BaseObject::new(tag_bytes.clone())?;
        // Get the hash of the created Git object
        let tag_hash = base_object.hash().to_owned();
        // Validate that storing the Git object bytes as a packfile base item is successful
        let result =
            upload_packfile_base_item(&ctx, &blobstore, &tag_hash, tag_bytes.to_vec()).await;
        assert!(result.is_ok());
        // Fetch the uploaded packfile base item and validate that it corresponds to the same base object
        let fetched_packfile_base_item =
            fetch_packfile_base_item(&ctx, &blobstore, &tag_hash).await?;
        let original_packfile_base_item: GitPackfileBaseItem = base_object.try_into()?;
        assert_eq!(fetched_packfile_base_item, original_packfile_base_item);
        anyhow::Ok(())
    }
}
