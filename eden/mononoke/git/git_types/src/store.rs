/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreBytes, BlobstoreGetData, Loadable, LoadableError, Storable};
use context::CoreContext;
use fbthrift::compact_protocol;
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

        impl TryFrom<BlobstoreGetData> for $ty {
            type Error = Error;

            fn try_from(blob: BlobstoreGetData) -> Result<Self, Error> {
                blob.into_bytes().try_into()
            }
        }

        impl Into<BlobstoreGetData> for $ty {
            fn into(self) -> BlobstoreGetData {
                Into::<BlobstoreBytes>::into(self).into()
            }
        }
    };
}

macro_rules! impl_loadable_storable {
    ($handle: ident, $ty:ident) => {
        #[async_trait]
        impl Storable for $ty {
            type Key = $handle;

            async fn store<B: Blobstore>(
                self,
                ctx: CoreContext,
                blobstore: &B,
            ) -> Result<Self::Key, Error> {
                let handle = *self.handle();
                let key = handle.blobstore_key();
                blobstore.put(ctx, key, self.into()).await?;
                Ok(handle)
            }
        }

        #[async_trait]
        impl Loadable for $handle {
            type Value = $ty;

            async fn load<'a, B: Blobstore>(
                &'a self,
                ctx: CoreContext,
                blobstore: &'a B,
            ) -> Result<Self::Value, LoadableError> {
                let id = *self;
                let get = blobstore.get(ctx, id.blobstore_key());
                let bytes = get.await?;
                match bytes {
                    Some(bytes) => bytes.try_into().map_err(LoadableError::Error),
                    None => Err(LoadableError::Missing(id.blobstore_key())),
                }
            }
        }

        impl_blobstore_conversions!($handle);
        impl_blobstore_conversions!($ty);
    };
}

impl_loadable_storable!(TreeHandle, Tree);
