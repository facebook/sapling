/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Storable;
use context::CoreContext;
use futures::future;
use mononoke_types::BlobstoreValue;
use mononoke_types::ContentAlias;
use mononoke_types::ContentMetadata;

use crate::errors::ErrorKind;
use crate::errors::InvalidHash;
use crate::fetch_key::Alias;
use crate::fetch_key::AliasBlob;
use crate::prepare::Prepared;
use crate::StoreRequest;

fn check_hash<T: std::fmt::Debug + PartialEq + Copy>(
    expected: Option<T>,
    effective: T,
) -> Result<(), InvalidHash<T>> {
    if let Some(expected) = expected {
        if expected != effective {
            return Err(InvalidHash {
                expected,
                effective,
            });
        }
    }

    Ok(())
}

pub async fn finalize<B: Blobstore>(
    blobstore: &B,
    ctx: &CoreContext,
    req: Option<&StoreRequest>,
    outcome: Prepared,
) -> Result<ContentMetadata, Error> {
    let Prepared {
        sha1,
        sha256,
        git_sha1,
        contents,
    } = outcome;

    let total_size = contents.size();

    let blob = contents.into_blob();
    let content_id = *blob.id();

    // If we were provided any hashes in the request, then validate them before we proceed.
    if let Some(req) = req {
        let StoreRequest {
            expected_size,
            canonical: req_content_id,
            sha1: req_sha1,
            sha256: req_sha256,
            git_sha1: req_git_sha1,
        } = req;

        expected_size.check_equals(total_size)?;

        {
            use ErrorKind::*;
            check_hash(*req_content_id, content_id).map_err(InvalidContentId)?;
            check_hash(*req_sha1, sha1).map_err(InvalidSha1)?;
            check_hash(*req_sha256, sha256).map_err(InvalidSha256)?;
            check_hash(*req_git_sha1, git_sha1).map_err(InvalidGitSha1)?;
        }
    }

    let alias = ContentAlias::from_content_id(content_id);
    let put_sha1 = AliasBlob(Alias::Sha1(sha1), alias.clone()).store(ctx, blobstore);
    let put_sha256 = AliasBlob(Alias::Sha256(sha256), alias.clone()).store(ctx, blobstore);
    let put_git_sha1 = AliasBlob(Alias::GitSha1(git_sha1.sha1()), alias).store(ctx, blobstore);

    // Since we don't have atomicity for multiple puts, we need to make sure they're ordered
    // correctly:
    //
    // - write the forward-mapping aliases
    // - write the data blob
    // - write the metadata blob
    //
    // Rationale for this order: since we can't guarantee the aliases are written atomically,
    // on failure we could end up writing some but not others. If the underlying blob exists
    // at that point, we've got an inconsistency. However writing the data blob is atomic,
    // and the aliases are only meaningful as references to that blob (in other words, an
    // alias referring to an absent blob is itself considered to be absent, so logically all
    // all the aliases come into existence atomically when the data blob is written).
    // Once the data blob is written we can write the metadata object. This is just a
    // cache, as everything in it can be computed from the content id. Therefore, in principle,
    // if it doesn't get written we can fix it up later.

    future::try_join3(put_sha1, put_sha256, put_git_sha1).await?;

    blob.store(ctx, blobstore).await?;

    let metadata = ContentMetadata {
        total_size,
        content_id,
        sha1,
        git_sha1,
        sha256,
    };

    metadata.clone().into_blob().store(ctx, blobstore).await?;

    Ok(metadata)
}
