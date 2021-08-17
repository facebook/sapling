/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
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
//! The `BonsaiDerivable` trait defines how derivation occurs.  Each derived
//! data type must implement `BonsaiDerivable` to describe how to derive
//! a new value from the inputs given above.
//!
//! As a performance enhancement, a derived data type may also implement the
//! `batch_derive` method of this trait to implement a fast path for deriving
//! data from a batch of changesets.  The default implementation derives each
//! changeset in the batch sequentially.
//!
//! The exact behaviour of derivation can be customized by an `Options` type
//! which is passed to each call of `derive_from_parents`.  This can be used,
//! for example, to derive different versions of the same data type.
//!
//! ### BonsaiDerivedMapping
//!
//! The `BonsaiDerivedMapping` trait defines storage of a mapping from bonsai
//! changeset IDs to derived data.  Once data has been derived, it will be
//! stored in the mapping so that it does not need to be derived again.
//!
//! The mapping also defines the derivation options that are used when
//! deriving data within that mapping.  For example, a unodes V1 mapping will
//! map changesets to their V1 root unodes, and a unodes V2 mapping map
//! changesets to their V2 root unodes.
//!
//! There are two utility traits to make implementing `BonsaiDerivedMapping`
//! simpler for the common cases.  These are located in the `mapping_impl`
//! module:
//!
//! * `BlobstoreRootIdMapping` maps changeset IDs to a root derived ID,
//!   stored in the blobstore.  This is useful for manifest-style derived
//!   data, where all that needs to be stored is a single blob pointing to a
//!   root ID, e.g. unodes, fsnodes or skeleton_manifests.
//!
//! * `BlobstoreExistsMapping` records that a changeset has been derived
//!   in the blobstore, but doesn't store any additional data.  This is
//!   useful for derived data types where it's sufficient to record that
//!   derivation has occurred, as all derived data can be found via
//!   other IDs, e.g. blame data, which is looked-up via unode ID.
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
//! repository is the methods of the `BonsaiDerived` trait.  For example:
//!
//! ```ignore
//! use derived_data::BonsaiDerived;
//!
//! let value = DerivedDataType::derive(ctx, repo, cs_id).await?;
//! ```
//!
//! This will obtain the default mapping for this derived data type for the
//! repository, and derive the value for that changeset.
//!
//! More complex derivations, for example when deriving data for many
//! changesets, can be performed using the `DerivedUtils` implementation
//! in the `derived_data_utils` crate.

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use context::{CoreContext, SessionClass};
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

pub mod batch;
pub mod derive_impl;
pub mod logging;
pub mod mapping;
pub mod mapping_impl;

pub use derive_impl::enabled_type_config;
pub use mapping::{BonsaiDerivedMapping, BonsaiDerivedMappingContainer, RegenerateMapping};
pub use mapping_impl::{
    BlobstoreExistsMapping, BlobstoreExistsWithDataMapping, BlobstoreRootIdMapping,
};
pub use metaconfig_types::DerivedDataTypesConfig;

#[derive(Debug, Error)]
pub enum DeriveError {
    #[error("Derivation of {0} is not enabled for repo={2} repoid={1}")]
    Disabled(&'static str, RepositoryId, String),
    #[error(transparent)]
    Error(#[from] Error),
}

/// Trait for defining how derived data is derived.  This trait should be
/// implemented by derivable data types.
#[async_trait]
pub trait BonsaiDerivable: Sized + 'static + Send + Sync + Clone + std::fmt::Debug {
    /// Name of derived data
    ///
    /// Should be unique string (among derived data types), which is used to identify or
    /// name data (for example lease keys) assoicated with particular derived data type.
    const NAME: &'static str;

    /// Type for additional options to derivation
    type Options: Send + Sync + 'static;

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        options: &Self::Options,
    ) -> Result<Self, Error> {
        let ctx = override_ctx(ctx, &repo);
        Self::derive_from_parents_impl(ctx, repo, bonsai, parents, options).await
    }

    /// Defines how to derive new representation for bonsai having derivations
    /// for parents and having a current bonsai object.
    ///
    /// Note that if any data has to be persistently stored in blobstore, mysql or any other store
    /// then it's responsiblity of implementor of `derive_from_parents_impl()` to save it.
    /// For example, to derive HgChangesetId we also need to derive all filenodes and all manifests
    /// and then store them in blobstore. Derived data library is only responsible for
    /// updating BonsaiDerivedMapping.
    async fn derive_from_parents_impl(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        options: &Self::Options,
    ) -> Result<Self, Error>;

    async fn batch_derive(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csids: Vec<ChangesetId>,
        mapping: &BonsaiDerivedMappingContainer<Self>,
        gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>, Error> {
        let ctx = &override_ctx(ctx.clone(), repo);
        Self::batch_derive_impl(ctx, repo, csids, mapping, gap_size).await
    }

    /// This method might be overridden by BonsaiDerivable implementors if there's a more efficient
    /// way to derive a batch of commits for a particular mapping.
    ///
    /// Note that the default implementation does not support gapped derivation, and will
    /// derive all items in the batch.
    async fn batch_derive_impl(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csids: Vec<ChangesetId>,
        mapping: &BonsaiDerivedMappingContainer<Self>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>, Error> {
        let mut res = HashMap::new();
        // The default implementation must derive sequentially with no
        // parallelism or concurrency, as dependencies between changesets may
        // cause O(n^2) derivations.
        for csid in csids {
            let derived = derive_impl::derive_impl::<Self>(ctx, repo, mapping, csid).await?;
            res.insert(csid, derived);
        }
        Ok(res)
    }
}

/// Trait for accessing data that can be derived from bonsai changesets, such
/// as Mercurial or Git changesets, unodes, fsnodes, skeleton manifests and
/// other derived data.
#[async_trait]
pub trait BonsaiDerived: Sized + 'static + Send + Sync + Clone + BonsaiDerivable {
    /// The default mapping type when deriving this data.
    type DefaultMapping: BonsaiDerivedMapping<Value = Self>;

    /// Get the default mapping associated with this derived data type.
    ///
    /// This is the usual mapping used to access this derived data type, using
    /// the repository config to configure data derivation.
    ///
    /// Returns an error if this derived data type is not enabled.
    fn default_mapping(
        ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<Self::DefaultMapping, DeriveError>;

    /// This function is the entrypoint for changeset derivation, it converts
    /// bonsai representation to derived one by calling derive_from_parents(), and saves mapping
    /// from csid -> BonsaiDerived in BonsaiDerivedMapping
    ///
    /// This function fails immediately if this type of derived data is not enabled for this repo.
    async fn derive(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: ChangesetId,
    ) -> Result<Self, DeriveError> {
        let mapping = BonsaiDerivedMappingContainer::new(
            ctx.fb,
            repo.name(),
            repo.get_derived_data_config().scuba_table.as_deref(),
            Arc::new(Self::default_mapping(ctx, repo)?),
        );
        derive_impl::derive_impl::<Self>(ctx, repo, &mapping, csid).await
    }

    /// Fetch the derived data in cases where we might not want to trigger derivation, e.g. when scrubbing.
    async fn fetch_derived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
    ) -> Result<Option<Self>, Error> {
        let mapping = BonsaiDerivedMappingContainer::new(
            ctx.fb,
            repo.name(),
            repo.get_derived_data_config().scuba_table.as_deref(),
            Arc::new(Self::default_mapping(ctx, repo)?),
        );
        derive_impl::fetch_derived::<Self>(ctx, csid, &mapping).await
    }

    /// Returns min(number of ancestors of `csid` to be derived, `limit`)
    ///
    /// This function fails immediately if derived data is not enabled for this repo.
    async fn count_underived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
        limit: u64,
    ) -> Result<u64, DeriveError> {
        let mapping = BonsaiDerivedMappingContainer::new(
            ctx.fb,
            repo.name(),
            repo.get_derived_data_config().scuba_table.as_deref(),
            Arc::new(Self::default_mapping(ctx, repo)?),
        );
        let underived = derive_impl::find_topo_sorted_underived::<Self, _>(
            ctx,
            repo,
            &mapping,
            Some(*csid),
            Some(limit),
        )
        .await?;
        Ok(underived.len() as u64)
    }

    async fn is_derived(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: &ChangesetId,
    ) -> Result<bool, DeriveError> {
        let count = Self::count_underived(&ctx, &repo, &csid, 1).await?;
        Ok(count == 0)
    }
}

pub fn override_ctx(mut ctx: CoreContext, repo: &BlobRepo) -> CoreContext {
    let use_bg_class =
        tunables::tunables().get_by_repo_derived_data_use_background_session_class(repo.name());
    if let Some(true) = use_bg_class {
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
        ctx
    } else {
        ctx
    }
}
