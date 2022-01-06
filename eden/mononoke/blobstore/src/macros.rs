/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[macro_export]
macro_rules! impl_blobstore_conversions {
    ($ty:ident, $thrift_ty:ty) => {
        impl $crate::private::TryFrom<$crate::private::BlobstoreBytes> for $ty {
            type Error = $crate::private::Error;

            fn try_from(
                bytes: $crate::private::BlobstoreBytes,
            ) -> Result<Self, $crate::private::Error> {
                let t: $thrift_ty =
                    $crate::private::compact_protocol::deserialize(bytes.as_bytes().as_ref())?;
                t.try_into()
            }
        }

        impl From<$ty> for $crate::private::BlobstoreBytes {
            fn from(other: $ty) -> Self {
                let thrift: $thrift_ty = other.into();
                let data = $crate::private::compact_protocol::serialize(&thrift);
                Self::from_bytes(data)
            }
        }

        impl $crate::private::TryFrom<$crate::private::BlobstoreGetData> for $ty {
            type Error = $crate::private::Error;

            fn try_from(
                blob: $crate::private::BlobstoreGetData,
            ) -> Result<Self, $crate::private::Error> {
                blob.into_bytes().try_into()
            }
        }

        impl From<$ty> for $crate::private::BlobstoreGetData {
            fn from(other: $ty) -> Self {
                Into::<$crate::private::BlobstoreBytes>::into(other).into()
            }
        }
    };
}

/// You can use this macro under the following conditions:
/// 1. handle_type needs to implement TryFrom<handle_thrift_type> and Into<handle_thrift_type>
/// 2. same for value_type and value_thrift_type
/// 3. value_type has method `fn handle(&self) -> handle_type`
/// 4. handle_type has method `fb blobstore_key(&self) -> String`
#[macro_export]
macro_rules! impl_loadable_storable {
    (
        handle_type => $handle: ident,
        handle_thrift_type => $thrift_handle: ident,
        value_type => $ty:ident,
        value_thrift_type => $thrift_ty: ident,
    ) => {
        #[$crate::private::async_trait]
        impl $crate::private::Storable for $ty {
            type Key = $handle;

            async fn store<'a, B: $crate::private::Blobstore>(
                self,
                ctx: &'a $crate::private::CoreContext,
                blobstore: &'a B,
            ) -> Result<Self::Key, $crate::private::Error> {
                let handle = *self.handle();
                let key = handle.blobstore_key();
                blobstore.put(ctx, key, self.into()).await?;
                Ok(handle)
            }
        }

        #[$crate::private::async_trait]
        impl $crate::private::Loadable for $handle {
            type Value = $ty;

            async fn load<'a, B: $crate::private::Blobstore>(
                &'a self,
                ctx: &'a $crate::private::CoreContext,
                blobstore: &'a B,
            ) -> Result<Self::Value, $crate::private::LoadableError> {
                let id = *self;
                let bytes = blobstore.get(ctx, &id.blobstore_key()).await?;
                match bytes {
                    Some(bytes) => bytes
                        .try_into()
                        .map_err($crate::private::LoadableError::Error),
                    None => Err($crate::private::LoadableError::Missing(id.blobstore_key())),
                }
            }
        }

        $crate::impl_blobstore_conversions!($handle, $thrift_handle);
        $crate::impl_blobstore_conversions!($ty, $thrift_ty);
    };
}
