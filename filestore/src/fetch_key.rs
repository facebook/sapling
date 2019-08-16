// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::{Blobstore, Storable};
use context::CoreContext;
use failure_ext::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{hash, ContentAlias, ContentId, MononokeId};

/// Key for fetching - we can access with any of the supported key types
#[derive(Debug, Clone)]
pub enum FetchKey {
    Canonical(ContentId),
    Sha1(hash::Sha1),
    Sha256(hash::Sha256),
    GitSha1(hash::GitSha1),
}

impl FetchKey {
    pub fn blobstore_key(&self) -> String {
        use FetchKey::*;

        match self {
            Canonical(contentid) => contentid.blobstore_key(),
            GitSha1(gitkey) => format!("alias.gitsha1.{}", gitkey.to_hex()),
            Sha1(sha1) => format!("alias.sha1.{}", sha1.to_hex()),
            Sha256(sha256) => format!("alias.sha256.{}", sha256.to_hex()),
        }
    }
}

pub struct AliasBlob(pub FetchKey, pub ContentAlias);

impl Storable for AliasBlob {
    type Key = FetchKey;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error> {
        let key = self.0;
        let alias = self.1;

        blobstore
            .put(ctx, key.blobstore_key(), alias.into_blob())
            .map(move |_| key)
            .boxify()
    }
}
