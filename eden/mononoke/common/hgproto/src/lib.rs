/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mercurial command protocol
//!
//! Mercurial has a set of commands which are implemented across at least two protocols:
//! SSH and HTTP. This module defines enums representing requests and responses for those
//! protocols, and a Tokio Service framework for them via a trait.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Mutex;

use bytes::Bytes;
use mercurial_types::HgChangesetId;
use mercurial_types::HgManifestId;

pub mod batch;
mod commands;
mod dechunker;
mod errors;
mod handler;
pub mod sshproto;

#[derive(Debug, Eq, PartialEq)]
pub enum Request {
    Batch(Vec<SingleRequest>),
    Single(SingleRequest),
}

impl Request {
    pub fn record_request(&self, record: &Mutex<Vec<String>>) {
        let mut record = record.lock().expect("lock poisoned");
        match *self {
            Request::Batch(ref batch) => record.extend(batch.iter().map(|s| s.name().into())),
            Request::Single(ref req) => record.push(req.name().into()),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SingleRequest {
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
    Heads,
    Hello,
    Listkeys {
        namespace: String,
    },
    ListKeysPatterns {
        namespace: String,
        patterns: Vec<String>,
    },
    Lookup {
        key: String,
    },
    Known {
        nodes: Vec<HgChangesetId>,
    },
    Unbundle {
        heads: Vec<String>,
    },
    UnbundleReplay {
        heads: Vec<String>,
        replaydata: String,
        respondlightly: bool,
    },
    Gettreepack(GettreepackArgs),
    StreamOutShallow {
        tag: Option<String>,
    },
}

impl SingleRequest {
    pub fn name(&self) -> &'static str {
        match *self {
            SingleRequest::Branchmap => "branchmap",
            SingleRequest::Capabilities => "capabilities",
            SingleRequest::ClientTelemetry { .. } => "clienttelemetry",
            SingleRequest::Debugwireargs { .. } => "debugwireargs",
            SingleRequest::Heads => "heads",
            SingleRequest::Hello => "hello",
            SingleRequest::Listkeys { .. } => "listkeys",
            SingleRequest::Lookup { .. } => "lookup",
            SingleRequest::Known { .. } => "known",
            SingleRequest::Unbundle { .. } => "unbundle",
            SingleRequest::UnbundleReplay { .. } => "unbundlereplay",
            SingleRequest::Gettreepack(_) => "gettreepack",
            SingleRequest::StreamOutShallow { .. } => "stream_out_shallow",
            SingleRequest::ListKeysPatterns { .. } => "listkeyspatterns",
        }
    }
}

/// The arguments that `gettreepack` accepts, in a separate struct for
/// the convenience of callers.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GettreepackArgs {
    /// The directory of the tree to send (including its subdirectories).
    pub rootdir: MPath,
    /// The manifest nodes of the specified root directory to send.
    pub mfnodes: Vec<HgManifestId>,
    /// The manifest nodes of the rootdir that are already on the client.
    pub basemfnodes: BTreeSet<HgManifestId>,
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
    Branchmap(HashMap<String, HashSet<HgChangesetId>>),
    Capabilities(Vec<String>),
    ClientTelemetry(String),
    Debugwireargs(Bytes),
    Heads(HashSet<HgChangesetId>),
    Hello(HashMap<String, Vec<String>>),
    Listkeys(HashMap<Vec<u8>, Vec<u8>>),
    ListKeysPatterns(BTreeMap<String, HgChangesetId>),
    Lookup(Bytes),
    Known(Vec<bool>),
    ReadyForStream,
    Unbundle(Bytes),
    Gettreepack(Bytes),
    StreamOutShallow(Bytes),
}

impl SingleResponse {
    /// Whether this represents a streaming response. Streaming responses don't have any framing.
    pub fn is_stream(&self) -> bool {
        use SingleResponse::*;

        match self {
            &ReadyForStream | &Unbundle(_) | &Gettreepack(_) | &StreamOutShallow(_) => true,
            _ => false,
        }
    }
}

pub use commands::HgCommandRes;
pub use commands::HgCommands;
pub use errors::ErrorKind;
pub use handler::HgProtoHandler;
use mononoke_types::path::MPath;
