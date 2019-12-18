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
//! When the sparse idmap gets asked to convert unknown id or slice, it goes
//! through the following flow to find the answer:
//!
//! - Id -> Slice: Id -> RequestLocationToSlice -> ResponseIdSlicePair -> Slice
//! - Slice -> Id: Slice -> RequestSliceToLocation -> ResponseIdSlicePair -> Id

use crate::idmap::IdMapLike;
use crate::segment::FirstAncestorConstraint;
use crate::{segment::Dag, Id, IdMap};
use anyhow::{format_err, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

// Request and Response structures -------------------------------------------

/// Request for locating slices (commit hashes) in a Dag.
/// Useful for converting slices to ids.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RequestSliceToLocation {
    #[serde(rename = "n")]
    pub slices: Vec<Box<[u8]>>,

    #[serde(rename = "h")]
    pub heads: Vec<Box<[u8]>>,
}

/// Request for converting locations to slices (commit hashes).
/// Useful for converting ids to slices.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RequestLocationToSlice {
    #[serde(rename = "p")]
    pub paths: Vec<AncestorPath>,
}

/// Response for converting slices to ids or converting slices to ids.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResponseIdSlicePair {
    // For converting Id -> Slice, the client provides AncestorPath, the server provides
    // Vec<Box<[u8]>>.
    //
    // For converting Slice -> Id, the client provides Box<[u8]>, the server provides
    // AncestorPath.
    #[serde(rename = "p")]
    pub path_slices: Vec<(AncestorPath, Vec<Box<[u8]>>)>,
}

/// The `n`-th first ancestor of `x`. `x~n` in hg revset syntax.
/// Usually, `x` is commonly known by the client and the server.
///
/// This can be seen as a kind of "location".
#[derive(Serialize, Deserialize, Clone)]
pub struct AncestorPath {
    #[serde(rename = "x")]
    pub x: Box<[u8]>,

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

// Slice -> Id, step 1: Slice -> RequestSliceToLocation
// Works on an incomplete IdMap, client-side.
impl<M: IdMapLike> Process<Vec<Box<[u8]>>, RequestSliceToLocation> for (&M, &Dag) {
    fn process(self, slices: Vec<Box<[u8]>>) -> Result<RequestSliceToLocation> {
        let map = &self.0;
        let dag = &self.1;
        // Only provides heads in the master group, since it's expected that the
        // non-master group is already locally known.
        let heads = dag
            .heads_ancestors(dag.master_group()?)?
            .iter()
            .map(|id| map.slice(id))
            .collect::<Result<Vec<Box<[u8]>>>>()?;
        Ok(RequestSliceToLocation { slices, heads })
    }
}

// Id -> Slice, step 1: Id -> RequestLocationToSlice
// Works on an incomplete IdMap, client-side.
impl<M: IdMapLike> Process<Vec<Id>, RequestLocationToSlice> for (&M, &Dag) {
    fn process(self, ids: Vec<Id>) -> Result<RequestLocationToSlice> {
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
                let x = map.slice(x)?;
                Ok(AncestorPath {
                    x,
                    n,
                    batch_size: 1,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(RequestLocationToSlice { paths })
    }
}

// Slice -> Id, step 2: RequestSliceToLocation -> ResponseIdSlicePair
// Works on a complete IdMap, server-side.
impl<M: IdMapLike> Process<RequestSliceToLocation, ResponseIdSlicePair> for (&M, &Dag) {
    fn process(self, request: RequestSliceToLocation) -> Result<ResponseIdSlicePair> {
        let _ = request;
        unimplemented!()
    }
}

// Id -> Slice, step 2: RequestLocationToSlice -> ResponseIdSlicePair
// Works on a complete IdMap, server-side.
impl<M: IdMapLike> Process<RequestLocationToSlice, ResponseIdSlicePair> for (&M, &Dag) {
    fn process(self, request: RequestLocationToSlice) -> Result<ResponseIdSlicePair> {
        let _ = request;
        unimplemented!()
    }
}

// Slice -> Id or Id -> Slice, step 3: Apply RequestSliceToLocation to a local IdMap.
// Works on an incomplete IdMap, client-side.
impl<'a> Process<&ResponseIdSlicePair, ()> for (&'a mut IdMap, &'a Dag) {
    fn process(self, response: &ResponseIdSlicePair) -> Result<()> {
        let _ = response;
        unimplemented!()
    }
}
