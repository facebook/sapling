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
extern crate futures;
#[macro_use]
extern crate tokio_io;

#[cfg(test)]
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

extern crate context;
extern crate futures_ext;
extern crate itertools;
extern crate mercurial;
extern crate mercurial_bundles;
extern crate mercurial_types;
extern crate revset;

#[cfg(test)]
extern crate mercurial_types_mocks;
// QuickCheck for randomized testing.
extern crate hooks;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Debug};
use std::sync::Mutex;

use bytes::Bytes;

use mercurial_types::{HgFileNodeId, HgNodeHash};

mod batch;
mod commands;
mod dechunker;
mod errors;
mod handler;
pub mod sshproto;

const MAX_NODES_TO_LOG: usize = 5;

#[derive(Debug, Eq, PartialEq)]
pub enum Request {
    Batch(Vec<SingleRequest>),
    Single(SingleRequest),
}

impl Request {
    pub fn record_request(&self, record: &Mutex<Vec<String>>) {
        let mut record = record.lock().expect("lock poisoned");
        match self {
            &Request::Batch(ref batch) => record.extend(batch.iter().map(|s| s.name().into())),
            &Request::Single(ref req) => record.push(req.name().into()),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SingleRequest {
    Between {
        pairs: Vec<(HgNodeHash, HgNodeHash)>,
    },
    Branchmap,
    Capabilities,
    ClientTelemetry {
        args: HashMap<Vec<u8>, Vec<u8>>,
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
        nodes: Vec<HgNodeHash>,
    },
    Unbundle {
        heads: Vec<String>,
    },
    Gettreepack(GettreepackArgs),
    Getfiles,
    StreamOutShallow,
    GetpackV1,
}

impl SingleRequest {
    pub fn name(&self) -> &'static str {
        match self {
            &SingleRequest::Between { .. } => "between",
            &SingleRequest::Branchmap => "branchmap",
            &SingleRequest::Capabilities => "capabilities",
            &SingleRequest::ClientTelemetry { .. } => "clienttelemetry",
            &SingleRequest::Debugwireargs { .. } => "debugwireargs",
            &SingleRequest::Getbundle(_) => "getbundle",
            &SingleRequest::Heads => "heads",
            &SingleRequest::Hello => "hello",
            &SingleRequest::Listkeys { .. } => "listkeys",
            &SingleRequest::Lookup { .. } => "lookup",
            &SingleRequest::Known { .. } => "known",
            &SingleRequest::Unbundle { .. } => "unbundle",
            &SingleRequest::Gettreepack(_) => "gettreepack",
            &SingleRequest::Getfiles => "getfiles",
            &SingleRequest::StreamOutShallow => "stream_out_shallow",
            &SingleRequest::GetpackV1 => "getpackv1",
        }
    }
}

/// The arguments that `getbundle` accepts, in a separate struct for
/// the convenience of callers.
#[derive(Eq, PartialEq)]
pub struct GetbundleArgs {
    /// List of space-delimited hex nodes of heads to retrieve
    pub heads: Vec<HgNodeHash>,
    /// List of space-delimited hex nodes that the client has in common with the server
    pub common: Vec<HgNodeHash>,
    /// Comma-delimited set of strings defining client bundle capabilities.
    pub bundlecaps: HashSet<Vec<u8>>,
    /// Comma-delimited list of strings of ``pushkey`` namespaces. For each namespace listed, a bundle2 part will be included with the content of that namespace.
    pub listkeys: Vec<Vec<u8>>,
    /// phases: Boolean indicating whether phases data is requested
    pub phases: bool,
}

impl Debug for GetbundleArgs {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let bcaps: HashSet<_> = self
            .bundlecaps
            .iter()
            .map(|s| String::from_utf8_lossy(&s))
            .collect();
        let listkeys: Vec<_> = self
            .listkeys
            .iter()
            .map(|s| String::from_utf8_lossy(&s))
            .collect();
        let heads: Vec<_> = self.heads.iter().take(MAX_NODES_TO_LOG).collect();
        let common: Vec<_> = self.common.iter().take(MAX_NODES_TO_LOG).collect();
        fmt.debug_struct("GetbundleArgs")
            .field("heads_len", &self.heads.len())
            .field("heads", &heads)
            .field("common_len", &self.common.len())
            .field("common", &common)
            .field("bundlecaps", &bcaps)
            .field("listkeys", &listkeys)
            .field("phases", &self.phases)
            .finish()
    }
}

/// The arguments that `gettreepack` accepts, in a separate struct for
/// the convenience of callers.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GettreepackArgs {
    /// The directory of the tree to send (including its subdirectories). Can be empty, that means
    /// "root of the repo".
    pub rootdir: Bytes,
    /// The manifest nodes of the specified root directory to send.
    pub mfnodes: Vec<HgNodeHash>,
    /// The manifest nodes of the rootdir that are already on the client.
    pub basemfnodes: Vec<HgNodeHash>,
    /// The fullpath (not relative path) of directories underneath
    /// the rootdir that should be sent.
    pub directories: Vec<Bytes>,
    /// The depth from the root that should be sent.
    pub depth: Option<usize>,
}

#[derive(Debug)]
pub enum Response {
    Batch(Vec<SingleResponse>),
    Single(SingleResponse),
}

#[derive(Debug)]
pub enum SingleResponse {
    Between(Vec<Vec<HgNodeHash>>),
    Branchmap(HashMap<String, HashSet<HgNodeHash>>),
    Capabilities(Vec<String>),
    ClientTelemetry(String),
    Debugwireargs(Bytes),
    Getbundle(Bytes),
    Heads(HashSet<HgNodeHash>),
    Hello(HashMap<String, Vec<String>>),
    Listkeys(HashMap<Vec<u8>, Vec<u8>>),
    Lookup(Bytes),
    Known(Vec<bool>),
    ReadyForStream,
    Unbundle(Bytes),
    Gettreepack(Bytes),
    Getfiles(Bytes),
    StreamOutShallow(Bytes),
    Getpackv1(Bytes),
}

impl SingleResponse {
    /// Whether this represents a streaming response. Streaming responses don't have any framing.
    pub fn is_stream(&self) -> bool {
        use SingleResponse::*;

        match self {
            &Getbundle(_) | &ReadyForStream | &Unbundle(_) | &Gettreepack(_)
            | &StreamOutShallow(_) | &Getpackv1(_) => true,
            _ => false,
        }
    }
}

pub use commands::{HgCommandRes, HgCommands};
pub use errors::{Error, ErrorKind, Result};
pub use handler::HgProtoHandler;
