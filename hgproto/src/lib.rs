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
#![feature(conservative_impl_trait)]

// Tokio/IO
extern crate bytes;
extern crate futures;
#[macro_use]
extern crate tokio_io;

#[macro_use]
extern crate slog;

// Errors
#[macro_use]
extern crate failure_ext as failure;

#[cfg(test)]
#[macro_use]
extern crate maplit;
#[macro_use]
extern crate nom;

extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
extern crate revset;

// QuickCheck for randomized testing.
#[cfg(test)]
#[macro_use]
extern crate quickcheck;

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Debug};

use bytes::Bytes;

use mercurial_types::NodeHash;

mod batch;
mod dechunker;
mod errors;
mod handler;
mod commands;
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
    Batch(Vec<SingleRequest>),
    Single(SingleRequest),
}

#[derive(Debug, Eq, PartialEq)]
pub enum SingleRequest {
    Between {
        pairs: Vec<(NodeHash, NodeHash)>,
    },
    Branchmap,
    Branches {
        nodes: Vec<NodeHash>,
    },
    Clonebundles,
    Capabilities,
    Changegroup {
        roots: Vec<NodeHash>,
    },
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
    Listkeys {
        namespace: String,
    },
    Lookup {
        key: String,
    },
    Known {
        nodes: Vec<NodeHash>,
    },
    Pushkey {
        namespace: String,
        key: String,
        old: NodeHash,
        new: NodeHash,
    },
    Streamout,
    Unbundle {
        heads: Vec<String>,
    },
    Gettreepack(GettreepackArgs),
    Getfiles,
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

/// The arguments that `gettreepack` accepts, in a separate struct for
/// the convenience of callers.
#[derive(Debug, Eq, PartialEq)]
pub struct GettreepackArgs {
    /// The directory of the tree to send (including its subdirectories). Can be empty, that means
    /// "root of the repo".
    pub rootdir: Bytes,
    /// The manifest nodes of the specified root directory to send.
    pub mfnodes: Vec<NodeHash>,
    /// The manifest nodes of the rootdir that are already on the client.
    pub basemfnodes: Vec<NodeHash>,
    ///  The fullpath (not relative path) of directories underneath
    /// the rootdir that should be sent.
    pub directories: Vec<Bytes>,
}

#[derive(Debug)]
pub enum Response {
    Batch(Vec<SingleResponse>),
    Single(SingleResponse),
}

#[derive(Debug)]
pub enum SingleResponse {
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
    ReadyForStream,
    Unbundle,
    Gettreepack(Bytes),
    Getfiles(Bytes),
}

impl SingleResponse {
    /// Whether this represents a streaming response. Streaming responses don't have any framing.
    pub fn is_stream(&self) -> bool {
        use SingleResponse::*;

        match self {
            &Getbundle(_) => true,
            &ReadyForStream => true,
            &Gettreepack(_) => true,
            _ => false,
        }
    }
}

pub use commands::{HgCommandRes, HgCommands};
pub use errors::{Error, ErrorKind, Result};
pub use handler::HgProtoHandler;
