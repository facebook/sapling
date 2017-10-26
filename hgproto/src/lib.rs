// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Mercurial command protocol
//!
//! Mercurial has a set of commands which are implemented across at least two protocols:
//! SSH and HTTP. This module defines enums representing requests and responses for those
//! protocols, and a Tokio Service framework for them via a trait.

#![deny(warnings)]

// Tokio/IO
extern crate bytes;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;
extern crate futures;

#[macro_use]
extern crate slog;

// Errors
#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate nom;
#[cfg(test)]
#[macro_use]
extern crate maplit;

extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;

// QuickCheck for randomized testing.
#[cfg(test)]
#[macro_use]
extern crate quickcheck;

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Debug};

use bytes::Bytes;

use mercurial_types::NodeHash;

mod batch;
mod errors;
mod service;
pub mod sshproto;

// result from `branches()`
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct BranchRes {
    top: NodeHash,
    node: NodeHash,
    p0: Option<NodeHash>,
    p1: Option<NodeHash>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Request {
    Batch { cmds: Vec<(Vec<u8>, Vec<u8>)> },
    Between { pairs: Vec<(NodeHash, NodeHash)> },
    Branchmap,
    Branches { nodes: Vec<NodeHash> },
    Clonebundles,
    Capabilities,
    Changegroup { roots: Vec<NodeHash> },
    Changegroupsubset {
        bases: Vec<NodeHash>,
        heads: Vec<NodeHash>,
    },
    Debugwireargs {
        one: Vec<u8>,
        two: Vec<u8>,
        all_args: HashMap<Vec<u8>, Vec<u8>>,
    },
    Getbundle(GetbundleArgs),
    Heads,
    Hello,
    Listkeys { namespace: String },
    Lookup { key: String },
    Known { nodes: Vec<NodeHash> },
    Pushkey {
        namespace: String,
        key: String,
        old: NodeHash,
        new: NodeHash,
    },
    Streamout,
    Unbundle { heads: Vec<String>, /* stream: Stream<Vec<u8>, Error> TBD: Stream */ },
}

/// The arguments that `getbundle` accepts, in a separate struct for
/// the convenience of callers.
#[derive(Eq, PartialEq)]
pub struct GetbundleArgs {
    pub heads: Vec<NodeHash>,
    pub common: Vec<NodeHash>,
    pub bundlecaps: Vec<Vec<u8>>,
    pub listkeys: Vec<Vec<u8>>,
}

impl Debug for GetbundleArgs {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let bcaps: Vec<_> = self.bundlecaps
            .iter()
            .map(|s| String::from_utf8_lossy(&s))
            .collect();
        let listkeys: Vec<_> = self.listkeys
            .iter()
            .map(|s| String::from_utf8_lossy(&s))
            .collect();
        fmt.debug_struct("GetbundleArgs")
            .field("heads", &self.heads)
            .field("common", &self.common)
            .field("bundlecaps", &bcaps)
            .field("listkeys", &listkeys)
            .finish()
    }
}

#[derive(Debug)]
pub enum Response {
    Batch(Vec<Bytes>),
    Between(Vec<Vec<NodeHash>>),
    Branchmap(HashMap<String, HashSet<NodeHash>>),
    Branches(Vec<BranchRes>),
    Clonebundles(String),
    Capabilities(Vec<String>),
    Changegroup,
    Changegroupsubset,
    Debugwireargs(Bytes),
    Getbundle(Bytes),
    Heads(HashSet<NodeHash>),
    Hello(HashMap<String, Vec<String>>),
    Listkeys(HashMap<Vec<u8>, Vec<u8>>),
    Lookup(NodeHash),
    Known(Vec<bool>),
    Pushkey,
    Streamout, /* (BoxStream<Vec<u8>, Error>) */
    Unbundle,
}

impl Response {
    /// Whether this represents a streaming response. Streaming responses don't have any framing.
    pub fn is_stream(&self) -> bool {
        use Response::*;

        match self {
            &Getbundle(_) => true,
            &Unbundle => true,
            _ => false,
        }
    }
}

pub use service::{HgCommandRes, HgCommands, HgService};
pub use errors::{Error, ErrorKind, Result, ResultExt};
