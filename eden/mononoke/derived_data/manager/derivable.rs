/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::TypeId;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures::future::try_join;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::context::DerivationContext;

use derived_data_service_if::types::DerivedData;

/// Defines how derivation occurs.  Each derived data type must implement
/// `BonsaiDerivable` to describe how to derive a new value from its inputs
/// and how to persist mappings from changesets to derived data.
///
/// As a performance enhancement, a derived data type may also implement the
/// `derive_batch` method of this trait to implement a fast path for deriving
/// data from a batch of changesets.  The default implementation derives each
/// changeset in the batch sequentially.
///
/// As a performance enhancement, a derived data type may also implement the
/// `fetch_batch` method of this trait to implement a fast path for fetching
/// previously-derived and persisted values for a batch of changesets.  The
/// default implementation fetches in separate requests.
#[async_trait]
pub trait BonsaiDerivable: Sized + Send + Sync + Clone + Debug + 'static {
    /// Name of the derived data type.
    ///
    /// Should be a unique string (among derived data types), which is used to
    /// identify or name data (for example lease keys) associated with this
    /// particular derived data type.
    const NAME: &'static str;

    /// Types of derived data types on which this derived data type
    /// depends.
    ///
    /// When performing derivation or backfilling, the derived data manager
    /// will ensure all dependencies are either derived or have been derived
    /// before deriving this derived data type.
    ///
    /// Use the `dependencies!` macro to populate this type.
    type Dependencies: DerivationDependencies;

    /// Derive data for a single changeset.
    ///
    /// If the implementation generates any other data (e.g. manifest nodes
    /// when deriving a manifest root), then it must ensure that all of this
    /// data has been written to its backing store before returning.
    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self>;

    /// Derive data for a batch of changesets.
    ///
    /// This method may be overridden by BonsaiDerivable implementors if
    /// there's a more efficient way to derive a batch of changesets.
    ///
    /// If a gap size is provided, then implementations may choose to
    /// derive only a subset of commits, as long as the gap between
    /// derived commits does not exceed the gap size.
    ///
    /// Not all implementations support gapped derivation, so some
    /// implementations may still derive all items in the batch.  The default
    /// implementation does not support gapped derivation.
    ///
    /// The batch is provided in topological order (ancestors appear before
    /// descendants).
    ///
    /// This method is called after the parents of the roots of the batch of
    /// changesets have been derived, as well as all the data for any of
    /// the dependent derived data types.  These data can be fetched from
    /// the derivation context.
    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        let mut res: HashMap<ChangesetId, Self> = HashMap::new();
        // The default implementation must derive sequentially with no
        // parallelism or concurrency, as dependencies between changesets may
        // cause O(n^2) derivations.
        for bonsai in bonsais {
            let csid = bonsai.get_changeset_id();
            let parents = derivation_ctx
                .fetch_unknown_parents(ctx, Some(&res), &bonsai)
                .await?;
            let derived = Self::derive_single(ctx, derivation_ctx, bonsai, parents).await?;
            res.insert(csid, derived);
        }
        Ok(res)
    }

    /// Store this derived data as the mapped value for a given changeset.
    ///
    /// Once derivation for a particular changeset is complete, this method
    /// is called so that the mapping from the changeset to the derived
    /// data type can be persisted, allowing it to be fetched again in the
    /// future.
    ///
    /// The derived data manager will ensure that any write caches provided to
    /// `derive_single` or `derive_batch` have been flushed before calling
    /// this method, so it is safe to persistently store the mapping
    /// immediately.
    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation: &DerivationContext,
        csid: ChangesetId,
    ) -> Result<()>;

    /// Fetch previously derived and persisted data.
    ///
    /// Returns None if the given changeset has not had derived data
    /// persisted previously.
    async fn fetch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        csid: ChangesetId,
    ) -> Result<Option<Self>>;

    /// Fetch a batch of previously derived data.
    ///
    /// This method may be overridden by BonsaiDerivable implementors if
    /// there's a more efficient way to fetch a batch of commits.
    ///
    /// Returns a hashmap containing only the commits which have been
    /// previously persisted.  Changesets for which derived data has not
    /// been previously persisted are omitted.
    async fn fetch_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Self>> {
        stream::iter(changeset_ids.iter().copied().map(|csid| async move {
            let maybe_derived = Self::fetch(ctx, derivation_ctx, csid).await?;
            Ok(maybe_derived.map(|derived| (csid, derived)))
        }))
        .buffer_unordered(64)
        .try_filter_map(|maybe_value| async move { Ok(maybe_value) })
        .try_collect()
        .await
    }

    fn from_thrift(_data: DerivedData) -> Result<Self>;

    fn into_thrift(_data: Self) -> Result<DerivedData>;
}

#[async_trait]
pub trait DerivationDependencies {
    /// Checks that all dependencies have been derived for this
    /// changeset.
    async fn check_dependencies(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        csid: ChangesetId,
        visited: &mut HashSet<TypeId>,
    ) -> Result<()>;
}

#[async_trait]
impl DerivationDependencies for () {
    async fn check_dependencies(
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
        _csid: ChangesetId,
        _visited: &mut HashSet<TypeId>,
    ) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl<Derivable, Rest> DerivationDependencies for (Derivable, Rest)
where
    Derivable: BonsaiDerivable,
    Rest: DerivationDependencies + 'static,
{
    async fn check_dependencies(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        csid: ChangesetId,
        visited: &mut HashSet<TypeId>,
    ) -> Result<()> {
        let type_id = TypeId::of::<Derivable>();
        if visited.insert(type_id) {
            try_join(
                derivation_ctx.fetch_dependency::<Derivable>(ctx, csid),
                Rest::check_dependencies(ctx, derivation_ctx, csid, visited),
            )
            .await?;
            Ok(())
        } else {
            Rest::check_dependencies(ctx, derivation_ctx, csid, visited).await
        }
    }
}

#[macro_export]
macro_rules! dependencies {
    () => { () };
    ($dep:ty) => { ( $dep , () ) };
    ($dep:ty, $( $rest:tt )*) => { ( $dep , dependencies!($( $rest )*) ) };
}
