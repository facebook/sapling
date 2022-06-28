/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Segmented Changelog Types

use std::collections::HashMap;

use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use auto_impl::auto_impl;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use thiserror::Error;

pub use dag;
pub use dag::CloneData;
pub use dag::FirstAncestorConstraint;
pub use dag::FlatSegment;
pub use dag::Group;
pub use dag::Id as DagId;
pub use dag::IdSet as DagIdSet;
pub use dag::InProcessIdDag;
pub use dag::Location;
pub use dag::PreparedFlatSegments;

#[facet::facet]
#[async_trait]
#[auto_impl(Arc)]
pub trait SegmentedChangelog: Send + Sync {
    /// Get the identifier of a commit given it's commit graph location.
    ///
    /// The client using segmented changelog will have only a set of identifiers for the commits in
    /// the graph. To retrieve the identifier of an commit that is now known they will provide a
    /// known descendant and the distance from the known commit to the commit we inquire about.
    async fn location_to_changeset_id(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
    ) -> Result<ChangesetId> {
        let mut ids = self
            .location_to_many_changeset_ids(ctx, location, 1)
            .await?;
        if ids.len() == 1 {
            if let Some(id) = ids.pop() {
                return Ok(id);
            }
        }
        Err(format_err!(
            "unexpected result from location_to_many_changeset_ids"
        ))
    }

    /// Get identifiers of a continuous set of commit given their commit graph location.
    ///
    /// Similar to `location_to_changeset_id` but instead of returning the ancestor that is
    /// `distance` away from the `known` commit, it returns `count` ancestors following the parents.
    /// It is expected that all but the last ancestor will have a single parent.
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>>;

    /// Get the graph location of a given commit identifier.
    ///
    /// ## Practical use-cases
    ///
    /// A lazy changelog client knows the "shape" of the commit graph where
    /// vertexes in the graph are labeled using numbers. It does not know
    /// all of the commit hashes corresponding to the numbers. When a commit
    /// hash was referred (ex. from user input), the client wants to "locate"
    /// the commit in its local graph, or be confident that the hash does not
    /// exist in its local graph.
    ///
    ///
    /// ## Principle (How it works)
    ///
    /// A lazy changelog client knows certain "anchor" commits (described as
    /// "universally known" in the `dag` crate), and can use those commits
    /// as "anchor points" to locate other commits. For example:
    ///
    /// The client wants to resolve C to its incomplete graph (only A is known).
    /// The server knows the client knows A's location.
    ///
    /// ```plain,ignore
    ///     Client | Server
    ///       A 30 |  A 55
    ///       |    |  |
    ///       ? 29 |  B 54
    ///       |    |  |
    ///       ? 28 |  C 53
    /// ```
    ///
    /// The server might use different integer IDs assigned in the graph so it
    /// cannot return C's server-side integer ID (53) directly. Instead, it
    /// translates `C` into `A~2` (revset notation, 1st parent of 1st parent of
    /// A), sends `A~2` to client, then client resolves `A~2` locally to integer
    /// ID 28.
    ///
    ///
    /// ## Heads
    ///
    /// To understand what "anchor" commits client has, this API requires
    /// `master_heads` from the client, meaning the client's lazy portion of
    /// the graph contains *exactly* `ancestors(master_heads)`. This indicates
    /// important properties like:
    ///
    /// 1. If the server returns `Ok(None)`, then the client can be confident
    ///    that the `cs_id` does not exist in the `ancestors(master_heads)`
    ///    sub-graph.
    /// 2. The server knows that `master_heads` and
    ///    `parents(ancestors(master_heads) & merge())` are "anchor"s known
    ///    by the client. The server should use those commit hashes as the `X`
    ///    part of `X~n` in responses.
    ///
    /// Providing precise heads is important. For example, suppose the client
    /// wants to resolve commit hash `X` with heads `Y`.  Then the server should
    /// return `Ok(None)` despite that the server knows `X`, because `X` is
    /// outside `ancestors(Y)`.
    ///
    /// ```plain,ignore
    ///     Client | Server
    ///            |  X
    ///            |  |
    ///       Y    |  Y
    ///       |    |  |
    ///       ?    |  Z
    /// ```
    ///
    /// Another example, commit `Z` can be represented as `Y~1`, `X~1`, or
    /// `Q~2`.  Client1 tries to resolve `Z` using heads `Y`, then the server
    /// should return `Y~1`, because `X` is unknown to client1. Client2 resolves
    /// `Z` using heads `Q`, then the server should return `Q~2`.
    ///
    /// ```plain,ignore
    ///     Client1 | Client2 | Server
    ///             | Q       | P Q
    ///             | |       | |\|
    ///     Y       | X       | Y X
    ///     |       | |       | |/
    ///     ?       | ?       | Z
    /// ```
    ///
    ///
    /// ## Multiple heads
    ///
    /// In a repo with multiple mainline branches, or when the unique master
    /// bookmark moves backwards, the client might send more than 1 head.
    /// The server's DAG might have more than 1 head too.
    ///
    /// Suppose the client provides two heads `A` and `B`.
    ///
    /// - If the server knows both `A` and `B`, great. Then the server can
    ///   process the request as usual.
    ///
    /// - If the server only knows `A` but does not know `B`, the server should
    ///   never return `Ok(None)`, because it cannot confirm that the `cs_id`
    ///   exists in `B % A` (revset notation) or not. This means the client can
    ///   can never confirm a commit hash does not exist in the graph, and
    ///   probably breaks a bunch of workflows until the client gets rid of the
    ///   troublesome heads (by rebuilding the graph).
    ///   If `cs_id` is an ancestor of `A`, the server can resolve it as if the client provides
    ///   only `A` as the head, or return an error (correct, but provides less optimal
    ///   UX when master moves backwards).
    async fn changeset_id_to_location(
        &self,
        ctx: &CoreContext,
        master_heads: Vec<ChangesetId>,
        cs_id: ChangesetId,
    ) -> Result<Option<Location<ChangesetId>>> {
        let mut ids = self
            .many_changeset_ids_to_locations(ctx, master_heads, vec![cs_id])
            .await?;
        ids.remove(&cs_id).transpose()
    }

    /// Get the graph locations given a set of commit identifier.
    ///
    /// Batch variation of `changeset_id_to_location`. The assumption is that we are dealing with
    /// the same client repository so the `head` parameter stays the same between changesets.
    ///
    /// See `changeset_id_to_location` for corner cases of this method.
    async fn many_changeset_ids_to_locations(
        &self,
        ctx: &CoreContext,
        master_heads: Vec<ChangesetId>,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Result<Location<ChangesetId>>>>;

    /// Returns data necessary for SegmentedChangelog to be initialized by a client.
    ///
    /// Note that the heads that are sent over in a clone can vary. Strictly speaking the client
    /// only needs one head.
    ///
    /// The HashMap is provided to contain hints for Mercurial clones - it has any pre-cached
    /// bonsai to hg mappings we have loaded as part of clone, and may be empty.
    async fn clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<(CloneData<ChangesetId>, HashMap<ChangesetId, HgChangesetId>)>;

    /// Returns data that client can import to perform a lazy fast pull.
    /// `common` specifies heads known by both client and server.
    /// `missing` specifies heads unknown by the client, and are being pulled.
    ///
    /// Note this endpoint does not handle all `pull` use-cases. It requires
    /// `roots(missing % common)` to be fully missing client-side, and
    /// `parents(roots(missing % common))` to exist client-side in its MASTER
    /// group. It's possible that client-side has `parents(roots(missing %
    /// common))` in its NON_MASTER group, for example, the client has some
    /// commits that are merged by the server. At present, that prevents the
    /// client from importing the pull data.
    async fn pull_data(
        &self,
        ctx: &CoreContext,
        common: Vec<ChangesetId>,
        missing: Vec<ChangesetId>,
    ) -> Result<CloneData<ChangesetId>>;

    /// Whether segmented changelog is disabled.
    ///
    /// A quick way to test if the backend supports segmented changelog or not
    /// without doing real work.
    ///
    /// Return true if it is disabled.
    async fn disabled(&self, ctx: &CoreContext) -> Result<bool>;

    /// Test if `ancestor` is an ancestor of `descendant`.
    /// Returns None in case segmented changelog doesn't know about either of those commit.
    async fn is_ancestor(
        &self,
        ctx: &CoreContext,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> Result<Option<bool>>;

    /// Try update segmented changelog to given heads. No-op by default. Useful
    /// for tests. Returns: `true` if update was successful; `false` if the
    /// implementation doesn't support updates; an error otherwise.
    async fn build_up_to_heads(&self, _ctx: &CoreContext, _heads: &[ChangesetId]) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Debug, Error)]
#[error("server cannot match the clients heads, repo {repo_id}, client_heads: {client_heads:?}")]
pub struct MismatchedHeadsError {
    pub repo_id: RepositoryId,
    pub client_heads: Vec<ChangesetId>,
}

impl MismatchedHeadsError {
    pub fn new(repo_id: RepositoryId, client_heads: Vec<ChangesetId>) -> Self {
        Self {
            repo_id,
            client_heads,
        }
    }
}
