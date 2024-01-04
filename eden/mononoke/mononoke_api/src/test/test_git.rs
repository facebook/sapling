/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use blobstore::Blobstore;
use fbinit::FacebookInit;
use filestore::hash_bytes;
use filestore::Sha1IncrementalHasher;
use git_types::GitError;
use gix_hash::ObjectId;
use gix_object::Tag;
use gix_object::WriteTo;

use crate::CoreContext;
use crate::RepoContext;

async fn init_repo(ctx: &CoreContext) -> Result<RepoContext> {
    let repo = test_repo_factory::build_empty(ctx.fb).await?;
    let repo_context = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok(repo_context)
}

/// upload_non_blob_git_object tests

#[fbinit::test]
/// Validate the basic git upload object functionality works.
async fn basic_upload_non_blob_git_object(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;
    let tag = Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    };
    let mut bytes = tag.loose_header().into_vec();
    tag.write_to(bytes.by_ref())?;

    let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), bytes.as_slice());
    let output = repo_ctx
        .upload_non_blob_git_object(gix_hash::oid::try_from_bytes(sha1_hash.as_ref())?, bytes)
        .await;
    output.expect("Expected git object to be uploaded successfully");
    Ok(())
}

#[fbinit::test]
/// Validate that we get an error while trying to upload a git blob through this method.
async fn blob_upload_non_blob_git_object(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;
    let blob = gix_object::Blob {
        data: "Some file content".as_bytes().to_vec(),
    };
    let mut bytes = blob.loose_header().into_vec();
    blob.write_to(bytes.by_ref())?;
    let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), bytes.as_slice());
    let output = repo_ctx
        .upload_non_blob_git_object(gix_hash::oid::try_from_bytes(sha1_hash.as_ref())?, bytes)
        .await;
    assert!(matches!(
        output.expect_err("Expected error during git object upload"),
        GitError::DisallowedBlobObject(_)
    ));
    Ok(())
}

#[fbinit::test]
/// Validate that we get an error while trying to upload invalid git bytes with this method.
async fn invalid_bytes_upload_non_blob_git_object(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;
    let tag = Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    };
    let mut bytes = Vec::new();
    tag.write_to(bytes.by_ref())?;

    let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), bytes.as_slice());
    let output = repo_ctx
        .upload_non_blob_git_object(gix_hash::oid::try_from_bytes(sha1_hash.as_ref())?, bytes)
        .await;
    assert!(matches!(
        output.expect_err("Expected error during git object upload"),
        GitError::InvalidContent(..)
    ));
    Ok(())
}

#[fbinit::test]
/// Validate that we get an error while trying to upload a git object with incorrect hash.
async fn invalid_hash_upload_non_blob_git_object(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;
    let tag = Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    };
    let mut bytes = tag.loose_header().into_vec();
    tag.write_to(bytes.by_ref())?;

    let output = repo_ctx
        .upload_non_blob_git_object(&ObjectId::empty_tree(gix_hash::Kind::Sha1), bytes)
        .await;
    assert!(matches!(
        output.expect_err("Expected error during git object upload"),
        GitError::HashMismatch(..)
    ));
    Ok(())
}

#[fbinit::test]
/// Validate that the git object stored in the blobstore is stored under the right key.
async fn blobstore_check_upload_non_blob_git_object(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;
    let tag = Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    };
    let mut bytes = tag.loose_header().into_vec();
    tag.write_to(bytes.by_ref())?;

    let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), bytes.as_slice());
    let blobstore_key = format!("git_object.{}", sha1_hash.to_hex());
    repo_ctx
        .upload_non_blob_git_object(gix_hash::oid::try_from_bytes(sha1_hash.as_ref())?, bytes)
        .await?;
    let output = repo_ctx.repo_blobstore().get(&ctx, &blobstore_key).await?;
    output.expect("Expected git object to be uploaded successfully");
    Ok(())
}

/// create_git_tree tests

#[fbinit::test]
/// Validate the basic create git tree functionality works.
async fn basic_create_git_tree(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;

    let tree = gix_object::Tree { entries: vec![] };
    let mut bytes = tree.loose_header().into_vec();
    tree.write_to(bytes.by_ref())?;

    let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), bytes.as_slice());
    let git_tree_hash = gix_hash::oid::try_from_bytes(sha1_hash.as_ref())?;
    repo_ctx
        .upload_non_blob_git_object(git_tree_hash, bytes)
        .await?;

    let output = repo_ctx.create_git_tree(git_tree_hash).await;
    output.expect("Expected git tree to be created successfully");
    Ok(())
}

#[fbinit::test]
/// Validate the create git tree method fails when the tree doesn't exist in Mononoke store.
async fn invalid_create_git_tree(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo_ctx = init_repo(&ctx).await?;
    let git_tree_hash = gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1);

    let output = repo_ctx.create_git_tree(&git_tree_hash).await;
    assert!(matches!(
        output.expect_err("Expected error during create git tree"),
        GitError::NonExistentObject(..)
    ));
    Ok(())
}
