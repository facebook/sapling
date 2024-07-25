/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # Derived Data
//!
//! This crate defines the traits that are used to implement data derivation
//! in Mononoke.
//!
//! Each type of derived data can be derived from a changeset when given:
//!   * The bonsai changeset (including its changeset ID).
//!   * The derived data of the same type for the immediate parent changesets.
//!
//! ## Traits
//!
//! ### BonsaiDerivable
//!
//! The `manager::BonsaiDerivable` trait defines how derivation occurs.  Each
//! derived data type must implement `BonsaiDerivable` to describe how to
//! derive a new value from the inputs given above.
//!
//! As a performance enhancement, a derived data type may also implement the
//! `derive_batch` method of this trait to implement a fast path for deriving
//! data from a batch of changesets.  The default implementation derives each
//! changeset in the batch sequentially.
//!
//! This trait also defines how derived data is stored and fetched via the
//! `store_mapping` and `fetch` method, which should store the data in a
//! way that can be fetched via its changeset id only.
//!
//! ### BonsaiDerived
//!
//! The `BonsaiDerived` trait ties these together by proving a default
//! mapping implementation that uses the configuration on the repository,
//! provided the derived data type is enabled for that repository.
//!
//! ## Usage
//!
//! The usual usage for deriving a particular derived data type in a
//! repository is the methods of `RepoDerivedData` or `DerivedDataManager`:
//!
//! ```ignore
//! use repo_derived_data::RepoDerivedDataRef;
//!
//! let value: DerivedDataType = repo.repo_derived_data().derive(ctx, cs_id).await?;
//! // Batch derivation
//! let manager = repo.repo_derived_data().manager();
//! manager.derive_exactly_batch::<DerivedDataType>(ctx, cs_ids.clone(), BatchDeriveOptions, None).await?;
//! let values: Vec<DerivedDataType> = manager.fetch_derived_batch(ctx, cs_ids, None).await?;
//! ```

use std::collections::HashMap;

use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use context::SessionClass;
use filestore::FetchKey;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use repo_identity::RepoIdentityRef;

pub mod batch;

pub use derived_data_manager::DerivationError;
pub use derived_data_manager::SharedDerivationError;
pub use metaconfig_types::DerivedDataTypesConfig;

pub mod macro_export {
    pub use anyhow::Error;
    pub use async_trait::async_trait;
    pub use context::CoreContext;
    pub use derived_data_manager::BonsaiDerivable;
    pub use mononoke_types::ChangesetId;
    pub use repo_derived_data::RepoDerivedDataRef;

    pub use super::DerivationError;
    pub use super::SharedDerivationError;
}

pub fn override_ctx(mut ctx: CoreContext, repo: impl RepoIdentityRef) -> CoreContext {
    let use_bg_class = justknobs::eval(
        "scm/mononoke:derived_data_use_background_session_class",
        None,
        Some(repo.repo_identity().name()),
    )
    .unwrap_or_default();
    if use_bg_class {
        ctx.session_mut()
            .override_session_class(SessionClass::BackgroundUnlessTooSlow);
        ctx
    } else {
        ctx
    }
}

/// Prefetch content metadata for a set of content ids.
pub async fn prefetch_content_metadata(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    content_ids: impl IntoIterator<Item = ContentId>,
) -> Result<HashMap<ContentId, ContentMetadataV2>> {
    stream::iter(content_ids)
        .map({
            move |content_id| {
                Ok(async move {
                    match filestore::get_metadata(blobstore, ctx, &FetchKey::Canonical(content_id))
                        .await?
                    {
                        Some(metadata) => Ok(Some((content_id, metadata))),
                        None => Ok(None),
                    }
                })
            }
        })
        .try_buffered(100)
        .try_filter_map(|maybe_metadata| async move { Ok(maybe_metadata) })
        .try_collect()
        .await
}
