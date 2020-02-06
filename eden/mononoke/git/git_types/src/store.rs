/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreBytes, Loadable, LoadableError, Storable};
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use std::convert::TryFrom;
use std::convert::TryInto;

use crate::{thrift, Tree, TreeHandle};

macro_rules! impl_blobstore_conversions {
    ($ty:ident) => {
        impl TryFrom<BlobstoreBytes> for $ty {
            type Error = Error;

            fn try_from(bytes: BlobstoreBytes) -> Result<Self, Error> {
                let t: thrift::$ty = compact_protocol::deserialize(bytes.as_bytes().as_ref())?;
                t.try_into()
            }
        }

        impl Into<BlobstoreBytes> for $ty {
            fn into(self) -> BlobstoreBytes {
                let thrift: thrift::$ty = self.into();
                let data = compact_protocol::serialize(&thrift);
                BlobstoreBytes::from_bytes(data)
            }
        }
    };
}

macro_rules! impl_loadable_storable {
    ($handle: ident, $ty:ident) => {
        impl Storable for $ty {
            type Key = $handle;

            fn store<B: Blobstore + Clone>(
                self,
                ctx: CoreContext,
                blobstore: &B,
            ) -> BoxFuture<Self::Key, Error> {
                let handle = *self.handle();
                let key = handle.blobstore_key();
                blobstore
                    .put(ctx, key, self.into())
                    .map(move |()| handle)
                    .boxify()
            }
        }

        impl Loadable for $handle {
            type Value = $ty;

            fn load<B: Blobstore + Clone>(
                &self,
                ctx: CoreContext,
                blobstore: &B,
            ) -> BoxFuture<Self::Value, LoadableError> {
                let id = *self;

                blobstore
                    .get(ctx, id.blobstore_key())
                    .from_err()
                    .and_then(move |bytes| match bytes {
                        Some(bytes) => bytes.try_into().map_err(LoadableError::Error),
                        None => Err(LoadableError::Missing(id.blobstore_key())),
                    })
                    .boxify()
            }
        }

        impl_blobstore_conversions!($handle);
        impl_blobstore_conversions!($ty);
    };
}

impl_loadable_storable!(TreeHandle, Tree);
