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

use crate::id::VertexName;
use crate::idmap::IdMapLike;
use crate::segment::FirstAncestorConstraint;
use crate::spanset::SpanSet;
use crate::{segment::Dag, Id, IdMap};
use anyhow::{format_err, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

// Request and Response structures -------------------------------------------

/// Request for locating names (commit hashes) in a Dag.
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

// Traits --------------------------------------------------------------------

/// Similar to `From::from(I) -> O`, but with `self` as context.
///
/// Example use-cases:
/// - Convert a query to a request (client-side).
/// - Convert a request to a response (server-side).
/// - Handle a response from the server (client-side).
pub(crate) trait Process<I, O> {
    fn process(self, input: I) -> Result<O>;
}

// Basic implementation ------------------------------------------------------

// Name -> Id, step 1: Name -> RequestNameToLocation
// Works on an incomplete IdMap, client-side.
impl<M: IdMapLike> Process<Vec<VertexName>, RequestNameToLocation> for (&M, &Dag) {
    fn process(self, names: Vec<VertexName>) -> Result<RequestNameToLocation> {
        let map = &self.0;
        let dag = &self.1;
        // Only provides heads in the master group, since it's expected that the
        // non-master group is already locally known.
        let heads = dag
            .heads_ancestors(dag.master_group()?)?
            .iter()
            .map(|id| map.vertex_name(id))
            .collect::<Result<Vec<VertexName>>>()?;
        Ok(RequestNameToLocation { names, heads })
    }
}

// Id -> Name, step 1: Id -> RequestLocationToName
// Works on an incomplete IdMap, client-side.
impl<M: IdMapLike> Process<Vec<Id>, RequestLocationToName> for (&M, &Dag) {
    fn process(self, ids: Vec<Id>) -> Result<RequestLocationToName> {
        let map = &self.0;
        let dag = &self.1;
        let heads = dag.heads_ancestors(dag.master_group()?)?;

        let paths = ids
            .into_iter()
            .map(|id| {
                let (x, n) = dag
                    .to_first_ancestor_nth(
                        id,
                        FirstAncestorConstraint::KnownUniversally {
                            heads: heads.clone(),
                        },
                    )?
                    .ok_or_else(|| format_err!("no segment for id {}", id))?;
                let x = map.vertex_name(x)?;
                Ok(AncestorPath {
                    x,
                    n,
                    batch_size: 1,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(RequestLocationToName { paths })
    }
}

// Name -> Id, step 2: RequestNameToLocation -> ResponseIdNamePair
// Works on a complete IdMap, server-side.
impl<M: IdMapLike> Process<RequestNameToLocation, ResponseIdNamePair> for (&M, &Dag) {
    fn process(self, request: RequestNameToLocation) -> Result<ResponseIdNamePair> {
        let map = &self.0;
        let dag = &self.1;
        let heads = request
            .heads
            .into_iter()
            .map(|s| map.vertex_id(s))
            .collect::<Result<Vec<Id>>>()?;
        let heads = SpanSet::from_spans(heads);
        let path_names = request
            .names
            .into_iter()
            .map(|name| -> Result<_> {
                let id = map.vertex_id(name.clone())?;
                let (x, n) = dag
                    .to_first_ancestor_nth(
                        id,
                        FirstAncestorConstraint::KnownUniversally {
                            heads: heads.clone(),
                        },
                    )?
                    .ok_or_else(|| format_err!("no path found for name {:?}", &name))?;
                let x = map.vertex_name(x)?;
                Ok((
                    AncestorPath {
                        x,
                        n,
                        batch_size: 1,
                    },
                    vec![name],
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ResponseIdNamePair { path_names })
    }
}

// Id -> Name, step 2: RequestLocationToName -> ResponseIdNamePair
// Works on a complete IdMap, server-side.
impl<M: IdMapLike> Process<RequestLocationToName, ResponseIdNamePair> for (&M, &Dag) {
    fn process(self, request: RequestLocationToName) -> Result<ResponseIdNamePair> {
        let map = &self.0;
        let dag = &self.1;
        let path_names = request
            .paths
            .into_iter()
            .map(|path| -> Result<_> {
                let id = map.vertex_id(path.x.clone())?;
                let mut id = dag.first_ancestor_nth(id, path.n)?;
                let names = (0..path.batch_size)
                    .map(|i| -> Result<VertexName> {
                        if i > 0 {
                            id = dag.first_ancestor_nth(id, 1)?;
                        }
                        let name = map.vertex_name(id)?;
                        Ok(name)
                    })
                    .collect::<Result<Vec<VertexName>>>()?;
                debug_assert_eq!(path.batch_size, names.len() as u64);
                Ok((path, names))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ResponseIdNamePair { path_names })
    }
}

// Name -> Id or Id -> Name, step 3: Apply RequestNameToLocation to a local IdMap.
// Works on an incomplete IdMap, client-side.
impl<'a> Process<&ResponseIdNamePair, ()> for (&'a mut IdMap, &'a Dag) {
    fn process(mut self, res: &ResponseIdNamePair) -> Result<()> {
        let map = &mut self.0;
        let dag = &self.1;
        for (path, names) in res.path_names.iter() {
            let x: Id = map
                .find_id_by_name(path.x.as_ref())?
                .ok_or_else(|| format_err!("server referred an unknown name {:?}", &path.x))?;
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
