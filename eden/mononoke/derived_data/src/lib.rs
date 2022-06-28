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
//! manager.backfill_batch::<DerivedDataType>(ctx, cs_ids.clone(), BatchDeriveOptions, None).await?;
//! let values: Vec<DerivedDataType> = manager.fetch_derived_batch(ctx, cs_ids, None).await?;
//! ```

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use context::CoreContext;
use context::SessionClass;
use mononoke_types::ChangesetId;

pub mod batch;

pub use derived_data_manager::DerivationError as DeriveError;
pub use metaconfig_types::DerivedDataTypesConfig;

pub mod macro_export {
    pub use super::BonsaiDerived;
    pub use super::DeriveError;
    pub use anyhow::Error;
    pub use async_trait::async_trait;
    pub use blobrepo::BlobRepo;
    pub use context::CoreContext;
    pub use derived_data_manager::BonsaiDerivable;
    pub use mononoke_types::ChangesetId;
    pub use repo_derived_data::RepoDerivedDataRef;
}

/// Trait for accessing data that can be derived from bonsai changesets, such
/// as Mercurial or Git changesets, unodes, fsnodes, skeleton manifests and
/// other derived data.
#[async_trait]
pub trait BonsaiDerived: Sized + Send + Sync + Clone + 'static {
    const DERIVABLE_NAME: &'static str;

    /// This function is the entrypoint for changeset derivation.  It will
    /// derive an instance of the derived data type based on the bonsai
    /// changeset representation.
    ///
    /// The derived data will be saved in a mapping from the changeset id,
    /// so that subsequent derives will just fetch.
    ///
    /// This function fails immediately if this type of derived data is not
    /// enabled for this repo.
    async fn derive(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: ChangesetId,
    ) -> Result<Self, DeriveError>;

    /// Fetch the derived data in cases where we might not want to trigger
    /// derivation, e.g. when scrubbing.
    async fn fetch_derived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
    ) -> Result<Option<Self>, Error>;

    /// Returns `true` if derived data has already been derived for this
    /// changeset.
    async fn is_derived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
    ) -> Result<bool, DeriveError> {
        Ok(Self::fetch_derived(ctx, repo, csid).await?.is_some())
    }

    /// Returns the number of ancestors of `csid` that are not yet derived,
    /// or at most `limit`.
    ///
    /// This function fails immediately if derived data is not enabled for
    /// this repo.
    async fn count_underived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
        limit: u64,
    ) -> Result<u64, DeriveError>;
}

#[macro_export]
macro_rules! impl_bonsai_derived_via_manager {
    ($derivable:ty) => {
        #[$crate::macro_export::async_trait]
        impl $crate::macro_export::BonsaiDerived for $derivable {
            const DERIVABLE_NAME: &'static str =
                <$derivable as $crate::macro_export::BonsaiDerivable>::NAME;

            async fn derive(
                ctx: &$crate::macro_export::CoreContext,
                repo: &$crate::macro_export::BlobRepo,
                csid: $crate::macro_export::ChangesetId,
            ) -> Result<Self, $crate::macro_export::DeriveError> {
                $crate::macro_export::RepoDerivedDataRef::repo_derived_data(repo)
                    .derive::<Self>(ctx, csid)
                    .await
            }

            async fn fetch_derived(
                ctx: &$crate::macro_export::CoreContext,
                repo: &$crate::macro_export::BlobRepo,
                csid: &$crate::macro_export::ChangesetId,
            ) -> Result<Option<Self>, $crate::macro_export::Error> {
                Ok(
                    $crate::macro_export::RepoDerivedDataRef::repo_derived_data(repo)
                        .fetch_derived::<Self>(ctx, *csid)
                        .await?,
                )
            }

            async fn count_underived(
                ctx: &$crate::macro_export::CoreContext,
                repo: &$crate::macro_export::BlobRepo,
                csid: &$crate::macro_export::ChangesetId,
                limit: u64,
            ) -> Result<u64, $crate::macro_export::DeriveError> {
                $crate::macro_export::RepoDerivedDataRef::repo_derived_data(repo)
                    .count_underived::<Self>(ctx, *csid, Some(limit))
                    .await
            }
        }
    };
}

pub fn override_ctx(mut ctx: CoreContext, repo: &BlobRepo) -> CoreContext {
    let use_bg_class =
        tunables::tunables().get_by_repo_derived_data_use_background_session_class(repo.name());
    if let Some(true) = use_bg_class {
        ctx.session_mut()
            .override_session_class(SessionClass::BackgroundUnlessTooSlow);
        ctx
    } else {
        ctx
    }
}
