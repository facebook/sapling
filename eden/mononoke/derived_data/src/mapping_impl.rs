/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};

use anyhow::{Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreBytes, BlobstoreGetData};
use context::CoreContext;
use futures::stream::{self, StreamExt, TryStreamExt};
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::ChangesetId;

use crate::{BonsaiDerivable, BonsaiDerived};

/// Implementation of a derived data mapping where the root id is stored
/// in the blobstore.
#[async_trait]
pub trait BlobstoreRootIdMapping {
    /// The mapped type that is stored in the blobstore.
    type Value: BonsaiDerived
        + TryFrom<BlobstoreGetData, Error = Error>
        + Into<BlobstoreBytes>
        + Send
        + Sync
        + Sized;

    /// Create a new instance of this mapping.
    fn new(repo: &BlobRepo, config: &DerivedDataTypesConfig) -> Result<Self>
    where
        Self: Sized;

    /// Returns the blobstore prefix to use for the mapping.
    fn prefix(&self) -> &'static str;

    /// Returns the blobstore that backs this mapping.
    fn blobstore(&self) -> &dyn Blobstore;

    /// Create a key for this mapping for a particular changeset.
    fn format_key(&self, cs_id: ChangesetId) -> String {
        format!("{}{}", self.prefix(), cs_id)
    }

    /// Fetch the corresponding value for a single changeset.
    async fn fetch(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<Self::Value>> {
        match self.blobstore().get(ctx, &self.format_key(cs_id)).await? {
            Some(blob) => Ok(Some(blob.try_into()?)),
            None => Ok(None),
        }
    }

    /// Fetch the corresponding value for a batch of changesets.
    async fn fetch_batch(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>> {
        stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            match self.fetch(ctx, cs_id).await? {
                Some(value) => Ok(Some((cs_id, value))),
                None => Ok(None),
            }
        }))
        .buffer_unordered(64)
        .try_filter_map(|maybe_value| async move { Ok(maybe_value) })
        .try_collect()
        .await
    }

    /// Store a new mapping value.
    async fn store(&self, ctx: &CoreContext, cs_id: ChangesetId, value: Self::Value) -> Result<()> {
        self.blobstore()
            .put(ctx, self.format_key(cs_id), value.into())
            .await
    }

    /// Fetch the options for this mapping implementation.
    fn options(&self) -> <Self::Value as BonsaiDerivable>::Options;
}

/// Implementation of a derived data mapping where the fact that derivation
/// has occurred is stored as an empty blob in the blobstore.
#[async_trait]
pub trait BlobstoreExistsMapping {
    type Value: BonsaiDerived + From<ChangesetId> + Send + Sync + Clone;

    /// Create a new instance of this mapping.
    fn new(repo: &BlobRepo, config: &DerivedDataTypesConfig) -> Result<Self>
    where
        Self: Sized;

    /// Returns the blobstore prefix to use for the mapping.
    fn prefix(&self) -> &'static str;

    /// Returns the blobstore that backs this mapping.
    fn blobstore(&self) -> &dyn Blobstore;

    /// Create a key for this mapping for a particular changeset.
    fn format_key(&self, cs_id: ChangesetId) -> String {
        format!("{}{}", self.prefix(), cs_id)
    }

    /// Returns whether a single changeset exists in the mapping.
    async fn exists(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<bool> {
        Ok(self
            .blobstore()
            .get(ctx, &self.format_key(cs_id))
            .await?
            .is_some())
    }

    /// Returns values for the changesets that exist in the mapping.
    async fn fetch_batch(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>> {
        stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if self.exists(ctx, cs_id).await? {
                Ok(Some((cs_id, cs_id.into())))
            } else {
                Ok(None)
            }
        }))
        .buffer_unordered(64)
        .try_filter_map(|maybe_value| async move { Ok(maybe_value) })
        .try_collect()
        .await
    }

    /// Stores a new entry in the mapping.
    async fn store(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        _value: Self::Value,
    ) -> Result<()> {
        self.blobstore()
            .put(ctx, self.format_key(cs_id), BlobstoreBytes::empty())
            .await
    }

    /// Fetch the options for this mapping implementation.
    fn options(&self) -> <Self::Value as BonsaiDerivable>::Options;
}

/// Macro to implement a bonsai derived mapping using a mapping implementation
/// type.
///
/// To implement `BonsaiDerivedMapping` for `ExampleMapping` using
/// `MappingImpl`:
///
/// ```ignore
/// struct ExampleMapping {
///     // Mapping definition
/// }
///
/// impl MappingImpl for ExampleMapping {
///      type Value = ExampleValue;
///
///     // Implement required methods for MappingImpl
/// }
///
/// // Tie the two together and implement BonsaiDerived and BonsaiDerivedMapping
/// impl_bonsai_derived_mapping!(ExampleMapping, MappingImpl, ExampleValue);
/// ```
#[macro_export]
macro_rules! impl_bonsai_derived_mapping {
    ($mapping:ident, $mapping_impl:ident, $value:ident) => {
        #[::async_trait::async_trait]
        impl $crate::BonsaiDerivedMapping for $mapping {
            type Value = $value;

            async fn get(
                &self,
                ctx: ::context::CoreContext,
                csids: ::std::vec::Vec<::mononoke_types::ChangesetId>,
            ) -> ::anyhow::Result<
                ::std::collections::HashMap<::mononoke_types::ChangesetId, Self::Value>,
            > {
                self.fetch_batch(&ctx, csids).await
            }

            async fn put(
                &self,
                ctx: ::context::CoreContext,
                csid: ::mononoke_types::ChangesetId,
                id: Self::Value,
            ) -> ::anyhow::Result<()> {
                self.store(&ctx, csid, id).await
            }

            fn options(&self) -> <Self::Value as $crate::BonsaiDerivable>::Options {
                <$mapping as $mapping_impl>::options(self)
            }
        }

        #[::async_trait::async_trait]
        impl $crate::BonsaiDerived for $value {
            type DefaultMapping = $mapping;

            fn default_mapping(
                _ctx: &::context::CoreContext,
                repo: &::blobrepo::BlobRepo,
            ) -> ::std::result::Result<Self::DefaultMapping, $crate::DeriveError> {
                let config = $crate::derive_impl::enabled_type_config(repo, Self::NAME)?;
                ::std::result::Result::Ok(<$mapping as $mapping_impl>::new(repo, config)?)
            }
        }
    };
}
