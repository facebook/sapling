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
use mononoke_types::{BlobstoreValue, ContentAlias, ContentMetadata, Storable};

use crate::errors::{ErrorKind, InvalidHash};
use crate::fetch_key::{Alias, AliasBlob};
use crate::prepare::Prepared;
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
    req: Option<&StoreRequest>,
    outcome: Prepared,
) -> impl Future<Item = ContentMetadata, Error = Error> {
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

        let _ = try_left_future!(expected_size.check_equals(total_size));

        {
            use ErrorKind::*;
            check_request_hash!(req_content_id, content_id, InvalidContentId);
            check_request_hash!(req_sha1, sha1, InvalidSha1);
            check_request_hash!(req_sha256, sha256, InvalidSha256);
            check_request_hash!(req_git_sha1, git_sha1, InvalidGitSha1);
        }
    }

    let put_contents = blob.store(ctx.clone(), &blobstore);

    let alias = ContentAlias::from_content_id(content_id);

    let put_sha1 = AliasBlob(Alias::Sha1(sha1), alias.clone()).store(ctx.clone(), &blobstore);

    let put_sha256 = AliasBlob(Alias::Sha256(sha256), alias.clone()).store(ctx.clone(), &blobstore);

    let put_git_sha1 =
        AliasBlob(Alias::GitSha1(git_sha1), alias.clone()).store(ctx.clone(), &blobstore);

    let metadata = ContentMetadata {
        total_size,
        content_id,
        sha1,
        git_sha1,
        sha256,
    };

    let put_metadata = metadata.clone().into_blob().store(ctx, &blobstore);

    // Since we don't have atomicity for puts, we need to make sure they're ordered
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

    (put_sha1, put_sha256, put_git_sha1)
        .into_future()
        .and_then(move |_| put_contents)
        .and_then(move |_| put_metadata)
        .map(move |_| metadata)
        .right_future()
}
