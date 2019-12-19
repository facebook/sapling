/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobstore::{Blobstore, Loadable, LoadableError, Storable};
use context::CoreContext;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{hash, ContentAlias, ContentId};

/// Key for fetching - we can access with any of the supported key types
#[derive(Debug, Clone)]
pub enum FetchKey {
    Canonical(ContentId),
    Aliased(Alias),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Alias {
    Sha1(hash::Sha1),
    Sha256(hash::Sha256),
    GitSha1(hash::GitSha1),
}

impl Loadable for FetchKey {
    type Value = ContentId;

    /// Return the canonical ID for a key. It doesn't check if the corresponding content actually
    /// exists (its possible for an alias to exist before the ID if there was an interrupted store
    /// operation).
    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        match self {
            FetchKey::Canonical(content_id) => Ok(*content_id).into_future().boxify(),
            FetchKey::Aliased(alias) => alias.load(ctx, blobstore),
        }
    }
}

impl Alias {
    pub fn blobstore_key(&self) -> String {
        match self {
            Alias::GitSha1(git_sha1) => format!("alias.gitsha1.{}", git_sha1.to_hex()),
            Alias::Sha1(sha1) => format!("alias.sha1.{}", sha1.to_hex()),
            Alias::Sha256(sha256) => format!("alias.sha256.{}", sha256.to_hex()),
        }
    }
}

impl Loadable for Alias {
    type Value = ContentId;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        let key = self.blobstore_key();
        blobstore
            .get(ctx, key.clone())
            .from_err()
            .and_then(move |maybe_alias| {
                let blob = maybe_alias.ok_or(LoadableError::Missing(key))?;

                ContentAlias::from_bytes(blob.into_bytes().into())
                    .map(|alias| alias.content_id())
                    .map_err(LoadableError::Error)
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
