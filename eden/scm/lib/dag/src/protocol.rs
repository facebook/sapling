/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

use crate::errors::NotFoundError;
use crate::id::VertexName;
use crate::iddag::{FirstAncestorConstraint, IdDag};
use crate::iddagstore::IdDagStore;
use crate::ops::IdConvert;
use crate::Error;
use crate::Group;
use crate::Id;
#[cfg(any(test, feature = "indexedlog-backend"))]
use crate::IdMap;
use crate::IdSet;
use crate::Result;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use serde::{Deserialize, Serialize};
use std::fmt;

// Request and Response structures -------------------------------------------

/// Request for locating names (commit hashes) in a IdDag.
/// Useful for converting names to ids.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RequestNameToLocation {
    #[serde(rename = "n")]
    pub names: Vec<VertexName>,

    #[serde(rename = "h")]
    pub heads: Vec<VertexName>,
}

/// Request for converting locations to names (commit hashes).
/// Useful for converting ids to names.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RequestLocationToName {
    #[serde(rename = "p")]
    pub paths: Vec<AncestorPath>,
}

/// Response for converting names to ids or converting names to ids.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResponseIdNamePair {
    // For converting Id -> Name, the client provides AncestorPath, the server provides
    // Vec<Box<[u8]>>.
    //
    // For converting Name -> Id, the client provides Box<[u8]>, the server provides
    // AncestorPath.
    #[serde(rename = "p")]
    pub path_names: Vec<(AncestorPath, Vec<VertexName>)>,
}

/// The `n`-th first ancestor of `x`. `x~n` in hg revset syntax.
/// Usually, `x` is commonly known by the client and the server.
///
/// This can be seen as a kind of "location".
#[derive(Serialize, Deserialize, Clone)]
pub struct AncestorPath {
    #[serde(rename = "x")]
    pub x: VertexName,

    #[serde(rename = "n")]
    pub n: u64,

    // Starting from x~n, get a chain of commits following p1.
    #[serde(rename = "c")]
    pub batch_size: u64,
}

impl fmt::Display for AncestorPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}~{}", self.x, self.n)
    }
}

impl fmt::Debug for AncestorPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
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
impl<M: IdConvert, DagStore: IdDagStore> Process<Vec<Id>, RequestLocationToName>
    for (&M, &IdDag<DagStore>)
{
    async fn process(self, ids: Vec<Id>) -> Result<RequestLocationToName> {
        let map = &self.0;
        let dag = &self.1;
        let heads = dag.heads_ancestors(dag.master_group()?)?;

        let ids = ids.into_iter();
        let x_ns = ids.map(|id| -> Result<(Id, u64)> {
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
            Ok((x, n))
        });
        let paths = stream::iter(x_ns)
            .and_then(|(x, n)| async move {
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
                Ok(AncestorPath {
                    x,
                    n,
                    batch_size: 1,
                })
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

        let id_names: Vec<(Id, VertexName)> = {
            let names = stream::iter(request.names.into_iter());
            names
                .then(|name| map.vertex_id(name.clone()).map_ok(|i| (i, name)))
                .try_collect()
                .await?
        };

        let path_names: Vec<(AncestorPath, Vec<VertexName>)> = {
            let x_n_names: Vec<(Id, u64, VertexName)> = id_names
                .into_iter()
                .map(|(id, name)| {
                    let (x, n) = dag
                        .to_first_ancestor_nth(
                            id,
                            FirstAncestorConstraint::KnownUniversally {
                                heads: heads.clone(),
                            },
                        )?
                        .ok_or_else(|| {
                            Error::Programming(format!(
                                "no x~n path found for {:?} ({})",
                                &name, id
                            ))
                        })?;
                    Ok((x, n, name))
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
                .then(|path| async {
                    let id = map.vertex_id(path.x.clone()).await?;
                    let mut id = dag.first_ancestor_nth(id, path.n)?;
                    let names: Vec<VertexName> = stream::iter(0..path.batch_size)
                        .then(|i| async move {
                            if i > 0 {
                                id = dag.first_ancestor_nth(id, 1)?;
                            }
                            let name = map.vertex_name(id).await?;
                            Ok::<_, crate::Error>(name)
                        })
                        .try_collect()
                        .await?;
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
        let map = &mut self.0;
        let dag = &self.1;
        for (path, names) in res.path_names.iter() {
            let x: Id = map
                .find_id_by_name(path.x.as_ref())?
                .ok_or_else(|| path.x.not_found_error())?;
            let mut id = dag.first_ancestor_nth(x, path.n)?;
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    id = dag.first_ancestor_nth(x, 1)?;
                }
                map.insert(id, name.as_ref())?;
            }
        }
        Ok(())
    }
}
