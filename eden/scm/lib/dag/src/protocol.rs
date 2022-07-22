/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # protocol
//!
//! Abstractions used for communication between `(sparse_idmap1, segments1)`
//! (usually, a client) and `(complete_idmap2, segments2)` (usually, a server).
//!
//! When the sparse idmap gets asked to convert unknown id or name, it goes
//! through the following flow to find the answer:
//!
//! - Id -> Name: Id -> RequestLocationToName -> ResponseIdNamePair -> Name
//! - Name -> Id: Name -> RequestNameToLocation -> ResponseIdNamePair -> Id

use std::cell::RefCell;
use std::fmt;
use std::thread_local;

use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use crate::id::VertexName;
use crate::iddag::FirstAncestorConstraint;
use crate::iddag::IdDag;
use crate::iddagstore::IdDagStore;
use crate::ops::IdConvert;
use crate::Group;
use crate::Id;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::IdMap;
use crate::IdSet;
use crate::Result;

// Request and Response structures -------------------------------------------

/// Request for locating names (commit hashes) in a IdDag.
/// Useful for converting names to ids.
#[derive(Debug, Clone)]
pub struct RequestNameToLocation {
    pub names: Vec<VertexName>,
    pub heads: Vec<VertexName>,
}

/// Request for converting locations to names (commit hashes).
/// Useful for converting ids to names.
#[derive(Debug, Clone)]
pub struct RequestLocationToName {
    pub paths: Vec<AncestorPath>,
}

/// Response for converting names to ids or converting names to ids.
#[derive(Debug, Clone)]
pub struct ResponseIdNamePair {
    // For converting Id -> Name, the client provides AncestorPath, the server provides
    // Vec<Box<[u8]>>.
    //
    // For converting Name -> Id, the client provides Box<[u8]>, the server provides
    // AncestorPath.
    pub path_names: Vec<(AncestorPath, Vec<VertexName>)>,
}

/// The `n`-th first ancestor of `x`. `x~n` in hg revset syntax.
/// Usually, `x` is commonly known by the client and the server.
///
/// This can be seen as a kind of "location".
#[derive(Clone)]
pub struct AncestorPath {
    pub x: VertexName,

    pub n: u64,

    // Starting from x~n, get a chain of commits following p1.
    pub batch_size: u64,
}

impl fmt::Display for AncestorPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}~{}", self.x, self.n)
    }
}

impl fmt::Debug for AncestorPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)?;
        if self.batch_size != 1 {
            write!(f, "(+{})", self.batch_size)?;
        }
        Ok(())
    }
}

// Async Remote Protocols ----------------------------------------------------

/// Abstraction of network protocols.
#[async_trait::async_trait]
pub trait RemoteIdConvertProtocol: Send + Sync + 'static {
    /// Ask the server to convert names to "x~n" relative paths.
    ///
    /// If a "name" cannot be resolved using "x~n" form in "::heads", aka. the
    /// "heads" are known to the server, and the server can calculate "::heads",
    /// and knows all names (commit hashes) in "::heads". And the server
    /// confirms "name" is outside "::heads" (either because "name" is unknown
    /// to the server's IdMap, or because "name" is known in the server's IdMap,
    /// but the matching Id is outside "::heads"), this method should skip it in
    /// the resulting list (instead of returning an error).
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<VertexName>,
        names: Vec<VertexName>,
    ) -> Result<Vec<(AncestorPath, Vec<VertexName>)>>;

    /// Ask the server to convert "x~n" relative paths back to commit hashes.
    ///
    /// Unlike resolve_names_to_relative_paths, failures are not expected.
    /// They usually indicate rare events like master moving backwards.
    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> Result<Vec<(AncestorPath, Vec<VertexName>)>>;

    /// Return `true` if the protocol is local and queries do not need to
    /// optimize for batching or latency.
    fn is_local(&self) -> bool {
        false
    }
}

#[async_trait::async_trait]
impl RemoteIdConvertProtocol for () {
    async fn resolve_names_to_relative_paths(
        &self,
        _heads: Vec<VertexName>,
        _names: Vec<VertexName>,
    ) -> Result<Vec<(AncestorPath, Vec<VertexName>)>> {
        Ok(Default::default())
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> Result<Vec<(AncestorPath, Vec<VertexName>)>> {
        let msg = format!(
            "Asked to resolve {:?} in graph but remote protocol is not configured",
            paths
        );
        crate::errors::programming(msg)
    }

    fn is_local(&self) -> bool {
        true
    }
}

// Traits --------------------------------------------------------------------

/// Similar to `From::from(I) -> O`, but with `self` as context.
///
/// Example use-cases:
/// - Convert a query to a request (client-side).
/// - Convert a request to a response (server-side).
/// - Handle a response from the server (client-side).
#[async_trait::async_trait]
pub(crate) trait Process<I, O> {
    async fn process(self, input: I) -> Result<O>;
}

// Basic implementation ------------------------------------------------------

// Name -> Id, step 1: Name -> RequestNameToLocation
// Works on an incomplete IdMap, client-side.
#[async_trait::async_trait]
impl<M: IdConvert, DagStore: IdDagStore> Process<Vec<VertexName>, RequestNameToLocation>
    for (&M, &IdDag<DagStore>)
{
    async fn process(self, names: Vec<VertexName>) -> Result<RequestNameToLocation> {
        let map = &self.0;
        let dag = &self.1;
        // Only provides heads in the master group, since it's expected that the
        // non-master group is already locally known.
        let heads = stream::iter(dag.heads_ancestors(dag.master_group()?)?.into_iter()).boxed();
        let heads = heads
            .then(|id| map.vertex_name(id))
            .try_collect::<Vec<VertexName>>()
            .await
            .map_err(|e| {
                let msg = format!(
                    concat!(
                        "Cannot resolve heads in master group to vertex name. ",
                        "The vertex name is required for remote vertex resolution. ",
                        "This probably indicates the Dag update logic does not ensure the ",
                        "vertex name of heads exist as it should. ",
                        "(Error: {})",
                    ),
                    e
                );
                crate::Error::Programming(msg)
            })?;
        Ok(RequestNameToLocation { names, heads })
    }
}

// Id -> Name, step 1: Id -> RequestLocationToName
// Works on an incomplete IdMap, client-side.
#[async_trait::async_trait]
impl<M: IdConvert, DagStore: IdDagStore> Process<IdSet, RequestLocationToName>
    for (&M, &IdDag<DagStore>)
{
    async fn process(self, ids: IdSet) -> Result<RequestLocationToName> {
        let map = &self.0;
        let dag = &self.1;
        let heads = dag.heads_ancestors(dag.master_group()?)?;

        let mut id_path: Vec<(Id, u64, u64)> = Vec::with_capacity(ids.as_spans().len());
        let mut last_id_opt = None;
        for id in ids.into_iter() {
            if let Some(last_id) = last_id_opt {
                if dag.try_first_ancestor_nth(last_id, 1)? == Some(id) {
                    // Reuse the last path.
                    if let Some(last) = id_path.last_mut() {
                        last.2 += 1;
                        last_id_opt = Some(id);
                        continue;
                    }
                }
            }
            let (x, n) = dag
                .to_first_ancestor_nth(
                    id,
                    FirstAncestorConstraint::KnownUniversally {
                        heads: heads.clone(),
                    },
                )?
                .ok_or_else(|| {
                    if id.group() == Group::MASTER {
                        let msg = format!(
                            concat!(
                                "Cannot convert {} to x~n form using heads {:?}. ",
                                "This is unexpected. It indicates some serious bugs in graph ",
                                "calculation or the graph is corrupted (ex. has cycles).",
                            ),
                            id, &heads,
                        );
                        crate::Error::Bug(msg)
                    } else {
                        let msg = format!(
                            concat!(
                                "Cannot convert {} to x~n form. This is unexpected for non-master ",
                                "vertexes since they are expected to be non-lazy.",
                            ),
                            id
                        );
                        crate::Error::Programming(msg)
                    }
                })?;
            id_path.push((x, n, 1));
            last_id_opt = Some(id);
        }

        let paths = stream::iter(id_path)
            .then(|(x, n, batch_size)| async move {
                let x = map.vertex_name(x).await.map_err(|e| {
                    let msg = format!(
                        concat!(
                            "Cannot resolve {} in to vertex name (Error: {}). ",
                            "The vertex name is required for remote vertex resolution. ",
                            "This probably indicates the Dag clone or update logic does ",
                            "not maintain \"universally known\" vertexes as it should.",
                        ),
                        x, e,
                    );
                    crate::Error::Programming(msg)
                })?;
                Ok::<_, crate::Error>(AncestorPath { x, n, batch_size })
            })
            .try_collect::<Vec<_>>()
            .await?;

        Ok(RequestLocationToName { paths })
    }
}

// Name -> Id, step 2: RequestNameToLocation -> ResponseIdNamePair
// Works on a complete IdMap, server-side.
#[async_trait::async_trait]
impl<M: IdConvert, DagStore: IdDagStore> Process<RequestNameToLocation, ResponseIdNamePair>
    for (&M, &IdDag<DagStore>)
{
    async fn process(self, request: RequestNameToLocation) -> Result<ResponseIdNamePair> {
        let map = &self.0;
        let dag = &self.1;

        let heads: IdSet = {
            let heads = stream::iter(request.heads.into_iter());
            let heads = heads
                .then(|s| map.vertex_id(s))
                .try_collect::<Vec<Id>>()
                .await?;
            IdSet::from_spans(heads)
        };
        let resolvable = dag.ancestors(heads.clone())?;

        let id_names: Vec<(Id, VertexName)> = {
            let ids_result = map.vertex_id_batch(&request.names).await?;
            let mut id_names = Vec::with_capacity(ids_result.len());
            for (name, id_result) in request.names.into_iter().zip(ids_result) {
                match id_result {
                    // If one of the names cannot be resolved to id, just skip it.
                    Err(crate::Error::VertexNotFound(n)) => {
                        tracing::trace!(
                            "RequestNameToLocation -> ResponseIdNamePair: skip unknown name {:?}",
                            &n
                        );
                        continue;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                    Ok(id) => {
                        if resolvable.contains(id) {
                            id_names.push((id, name))
                        }
                    }
                }
            }
            id_names
        };

        let path_names: Vec<(AncestorPath, Vec<VertexName>)> = {
            let x_n_names: Vec<(Id, u64, VertexName)> = id_names
                .into_iter()
                .filter_map(|(id, name)| {
                    match dag.to_first_ancestor_nth(
                        id,
                        FirstAncestorConstraint::KnownUniversally {
                            heads: heads.clone(),
                        },
                    ) {
                        Err(e) => Some(Err(e)),
                        // Skip ids that cannot be translated.
                        Ok(None) => None,
                        Ok(Some((x, n))) => Some(Ok((x, n, name))),
                    }
                })
                .collect::<Result<Vec<_>>>()?;

            // Convert x from Id to VertexName.
            stream::iter(x_n_names)
                .then(|(x, n, name)| async move {
                    let x = map.vertex_name(x).await?;
                    Ok::<_, crate::Error>((
                        AncestorPath {
                            x,
                            n,
                            batch_size: 1,
                        },
                        vec![name],
                    ))
                })
                .try_collect()
                .await?
        };

        Ok(ResponseIdNamePair { path_names })
    }
}

// Id -> Name, step 2: RequestLocationToName -> ResponseIdNamePair
// Works on a complete IdMap, server-side.
#[async_trait::async_trait]
impl<M: IdConvert, DagStore: IdDagStore> Process<RequestLocationToName, ResponseIdNamePair>
    for (&M, &IdDag<DagStore>)
{
    async fn process(self, request: RequestLocationToName) -> Result<ResponseIdNamePair> {
        let map = &self.0;
        let dag = &self.1;

        let path_names: Vec<(AncestorPath, Vec<VertexName>)> =
            stream::iter(request.paths.into_iter())
                .then(|path| async move {
                    let id = map.vertex_id(path.x.clone()).await?;
                    let mut id = dag.first_ancestor_nth(id, path.n)?;
                    let mut ids = Vec::with_capacity(path.batch_size as _);
                    for i in 0..path.batch_size {
                        if i > 0 {
                            id = dag.first_ancestor_nth(id, 1)?;
                        }
                        ids.push(id);
                    }
                    let fallible_names = map.vertex_name_batch(&ids).await?;
                    let mut names = Vec::with_capacity(fallible_names.len());
                    for name in fallible_names {
                        names.push(name?);
                    }
                    debug_assert_eq!(path.batch_size, names.len() as u64);
                    Ok::<_, crate::Error>((path, names))
                })
                .try_collect()
                .await?;
        Ok(ResponseIdNamePair { path_names })
    }
}

// Name -> Id or Id -> Name, step 3: Apply RequestNameToLocation to a local IdMap.
// Works on an incomplete IdMap, client-side.
#[cfg(any(test, feature = "indexedlog-backend"))]
#[async_trait::async_trait]
impl<'a, DagStore: IdDagStore> Process<ResponseIdNamePair, ()>
    for (&'a mut IdMap, &'a IdDag<DagStore>)
{
    async fn process(mut self, res: ResponseIdNamePair) -> Result<()> {
        use crate::errors::NotFoundError;

        let map = &mut self.0;
        let dag = &self.1;
        for (path, names) in res.path_names.iter() {
            let x: Id = map
                .find_id_by_name(path.x.as_ref())?
                .ok_or_else(|| path.x.not_found_error())?;
            let mut id = dag.first_ancestor_nth(x, path.n)?;
            tracing::trace!("insert path {:?} names {:?} (x = {})", &path, &names, id);
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    id = dag.first_ancestor_nth(id, 1)?;
                }
                tracing::trace!(" insert {:?} = {:?}", id, &name);
                map.insert(id, name.as_ref())?;
            }
        }
        Ok(())
    }
}

// Disable remote protocol temporarily ---------------------------------------
// This can be useful for Debug::fmt to disable remote fetching which might
// panic (ex. calling tokio without tokio runtime) when executing futures
// via nonblocking.

thread_local! {
    static NON_BLOCKING_DEPTH: RefCell<usize> = RefCell::new(0);
}

/// Check if the current future is running inside a "non-blocking" block.
pub(crate) fn disable_remote_protocol<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    NON_BLOCKING_DEPTH.with(|v| *v.borrow_mut() += 1);
    let result = f();
    NON_BLOCKING_DEPTH.with(|v| *v.borrow_mut() -= 1);
    result
}

pub(crate) fn is_remote_protocol_disabled() -> bool {
    NON_BLOCKING_DEPTH.with(|v| *v.borrow() != 0)
}
