// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::{Blobstore, Loadable, Storable};
use context::CoreContext;
use failure_ext::Error;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{hash, ContentAlias, ContentId};

/// Key for fetching - we can access with any of the supported key types
#[derive(Debug, Clone)]
pub enum FetchKey {
    Canonical(ContentId),
    Aliased(Alias),
}

#[derive(Debug, Clone)]
pub enum Alias {
    Sha1(hash::Sha1),
    Sha256(hash::Sha256),
    GitSha1(hash::GitSha1),
}

impl Loadable for FetchKey {
    type Value = Option<ContentId>;

    /// Return the canonical ID for a key. It doesn't check if the corresponding content actually
    /// exists (its possible for an alias to exist before the ID if there was an interrupted store
    /// operation).
    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, Error> {
        match self {
            FetchKey::Canonical(content_id) => Ok(Some(*content_id)).into_future().boxify(),
            FetchKey::Aliased(alias) => alias.load(ctx, blobstore),
        }
    }
}

impl Alias {
    fn blobstore_key(&self) -> String {
        match self {
            Alias::GitSha1(git_sha1) => format!("alias.gitsha1.{}", git_sha1.to_hex()),
            Alias::Sha1(sha1) => format!("alias.sha1.{}", sha1.to_hex()),
            Alias::Sha256(sha256) => format!("alias.sha256.{}", sha256.to_hex()),
        }
    }
}

impl Loadable for Alias {
    type Value = Option<ContentId>;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, Error> {
        blobstore
            .get(ctx, self.blobstore_key())
            .and_then(|maybe_alias| {
                maybe_alias
                    .map(|blob| {
                        ContentAlias::from_bytes(blob.into_bytes().into())
                            .map(|alias| alias.content_id())
                    })
                    .transpose()
            })
            .boxify()
    }
}

pub struct AliasBlob(pub Alias, pub ContentAlias);

impl Storable for AliasBlob {
    type Key = ();

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error> {
        blobstore.put(ctx, self.0.blobstore_key(), self.1.into_blob())
    }
}
