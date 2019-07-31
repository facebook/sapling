// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use context::CoreContext;
use failure_ext::Error;
use futures::{Future, IntoFuture};
use futures_ext::{try_left_future, FutureExt};
use mononoke_types::{
    BlobstoreBytes, BlobstoreValue, ContentAlias, ContentId, ContentMetadata, ContentMetadataId,
    MononokeId,
};

use crate::errors::{ErrorKind, InvalidHash};
use crate::prepare::Prepared;
use crate::FetchKey;
use crate::StoreRequest;

// Verify that a given $expected hash matches the $effective hash, and otherwise return a left
// future containing the $error.
macro_rules! check_request_hash {
    ($expected:expr, $effective:expr, $error:expr) => {
        if let Some(expected) = $expected {
            if *expected != $effective {
                return Err($error(InvalidHash {
                    expected: *expected,
                    effective: $effective,
                })
                .into())
                .into_future()
                .left_future();
            }
        }
    };
}

pub fn finalize<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    req: &StoreRequest,
    outcome: Prepared,
) -> impl Future<Item = ContentId, Error = Error> {
    let StoreRequest {
        expected_size,
        canonical: req_content_id,
        sha1: req_sha1,
        sha256: req_sha256,
        git_sha1: req_git_sha1,
    } = req;

    let Prepared {
        total_size,
        sha1,
        sha256,
        git_sha1,
        contents,
    } = outcome;

    let _ = try_left_future!(expected_size.check_equals(total_size));

    let blob = contents.into_blob();
    let content_id = *blob.id();

    // If we were provided any hashes in the request, then validate them before we proceed.
    {
        use ErrorKind::*;
        check_request_hash!(req_content_id, content_id, InvalidContentId);
        check_request_hash!(req_sha1, sha1, InvalidSha1);
        check_request_hash!(req_sha256, sha256, InvalidSha256);
        check_request_hash!(req_git_sha1, git_sha1, InvalidGitSha1);
    }

    let alias = ContentAlias::from_content_id(content_id).into_blob();

    let put_contents = blobstore.put(
        ctx.clone(),
        content_id.blobstore_key(),
        BlobstoreBytes::from(blob),
    );

    let put_sha1 = blobstore.put(
        ctx.clone(),
        FetchKey::Sha1(sha1).blobstore_key(),
        alias.clone(),
    );

    let put_sha256 = blobstore.put(
        ctx.clone(),
        FetchKey::Sha256(sha256).blobstore_key(),
        alias.clone(),
    );

    let put_git_sha1 = blobstore.put(
        ctx.clone(),
        FetchKey::GitSha1(git_sha1).blobstore_key(),
        alias.clone(),
    );

    let put_metadata = {
        let metadata = ContentMetadata {
            total_size,
            content_id,
            sha1: Some(sha1),
            git_sha1: Some(git_sha1),
            sha256: Some(sha256),
        };

        let blob = metadata.into_blob();
        let key = ContentMetadataId::from(content_id);
        blobstore.put(ctx, key.blobstore_key(), BlobstoreBytes::from(blob))
    };

    // Since we don't have atomicity for puts, we need to make sure they're ordered
    // correctly:
    //
    // - write the forward-mapping aliases
    // - write the data blob
    // - write the back-mapping blob
    //
    // Rationale for this order: since we can't guarantee the aliases are written atomically,
    // on failure we could end up writing some but not others. If the underlying blob exists
    // at that point, we've got an inconsistency. However writing the data blob is atomic,
    // and the aliases are only meaningful as references to that blob (in other words, an
    // alias referring to an absent blob is itself considered to be absent, so logically all
    // all the aliases come into existence atomically when the data blob is written).
    // Once the data blob is written we can write the back-mapping object. This is just a
    // cache, as everything in it can be computed from the content id. Therefore, in principle,
    // if it doesn't get written we can fix it up later.

    (put_sha1, put_sha256, put_git_sha1)
        .into_future()
        .and_then(move |_| put_contents)
        .and_then(move |_| put_metadata)
        .map(move |_| content_id)
        .right_future()
}
