/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Mercurial Types
//!
//! This crate contains useful definitions for types that occur in Mercurial. Or more generally,
//! in a source control system that is based on Mercurial and extensions.
//!
//! The top-most level is the Repo, which is a container for changesets.
//!
//! A changeset represents a snapshot of a file tree at a specific moment in time. Changesets
//! can (and commonly do) have parent-child relationships with other changesets; if once changeset
//! is the child of another one, then it is interpreted as an incremental change in the history of
//! a single namespace. Changesets can have multiple parents (currently limited to 2), which
//! represents the merging of history. A changeset can have no parents, which represents the
//! creation of a new namespace. There's no requirement that all (or any) changeset within a
//! repo be connected at all via parent-child relationships.
//!
//! Each changeset has a tree of manifests, which represent their namespace. A manifest is
//! equivalent to a directory in a filesystem, mapping names to other objects. Those other
//! objects can be other manifests (subdirectories), files, or symlinks. Manifest objects can
//! be shared by multiple changesets - if the only difference between two changesets is a
//! single file, then all other files and directories will be the same and shared.
//!
//! Changesets, manifests and files are uniformly represented by a `Node`. A `Node` has
//! 0-2 parents and some content. A node's identity is computed by hashing over (p1, p2, content),
//! resulting in `HgNodeHash` (TODO: rename HgNodeHash -> NodeId?). This means manifests and files
//! have a notion of history independent of the changeset(s) they're embedded in.
//!
//! Nodes are stored as blobs in the blobstore, but with their content in a separate blob. This
//! is because it's very common for the same file content to appear either under different names
//! (copies) or multiple times within the same history (reverts), or both (rebase, amend, etc).
//!
//! Blobs are the underlying raw storage for all immutable objects in Mononoke. Their primary
//! storage key is a hash (TBD, stronger than SHA1) over their raw bit patterns, but they can
//! have other keys to allow direct access via multiple aliases. For example, file content may be
//! shared by multiple nodes, but can be access directly without having to go via a node.
//!
//! Delta and bdiff are used in revlogs and on the wireprotocol to represent inter-file
//! differences. These are for interfacing at the edges, but are not used within Mononoke's core
//! structures at all.
#![deny(warnings)]
#![feature(const_fn)]

pub mod bdiff;
pub mod blob;
pub mod blobnode;
pub mod blobs;
pub mod changeset;
pub mod delta;
pub mod delta_apply;
mod envelope;
pub mod errors;
pub mod file;
pub mod flags;
pub mod fsencode;
pub mod globalrev;
pub mod hash;
pub mod manifest;
mod node;
pub mod nodehash;
pub mod phase;
pub mod remotefilelog;
pub mod sql_types;
pub mod utils;

pub use self::manifest::{HgEntry, HgManifest, Type};
pub use blob::HgBlob;
pub use blobnode::{calculate_hg_node_id, calculate_hg_node_id_stream, HgBlobNode, HgParents};
pub use blobs::{fetch_manifest_envelope, fetch_raw_manifest_bytes, fetch_raw_revlog_metadata};
pub use changeset::Changeset;
pub use delta::Delta;
pub use envelope::{
    HgChangesetEnvelope, HgChangesetEnvelopeMut, HgFileEnvelope, HgFileEnvelopeMut,
    HgManifestEnvelope, HgManifestEnvelopeMut,
};
pub use errors::ErrorKind;
pub use flags::{parse_rev_flags, RevFlags};
pub use fsencode::{fncache_fsencode, simple_fsencode};
// Re-exports from mononoke_types. Eventually these should go away and everything should depend
// directly on mononoke_types;
pub use file::FileBytes;
pub use globalrev::Globalrev;
pub use mononoke_types::{FileType, MPath, MPathElement, RepoPath};
pub use node::Node;
pub use nodehash::{
    HgChangesetId, HgEntryId, HgFileNodeId, HgManifestId, HgNodeHash, HgNodeKey, NULL_CSID,
    NULL_HASH,
};
pub use phase::HgPhase;
pub use remotefilelog::{convert_parents_to_remotefilelog_format, HgFileHistoryEntry};
pub use utils::percent_encode;

#[cfg(test)]
mod test;

mod thrift {
    pub use mercurial_thrift::*;
    pub use mononoke_types_thrift::*;
}

impl asyncmemo::Weight for HgChangesetId {
    fn get_weight(&self) -> usize {
        std::mem::size_of::<HgChangesetId>()
    }
}

impl asyncmemo::Weight for HgFileNodeId {
    fn get_weight(&self) -> usize {
        std::mem::size_of::<HgFileNodeId>()
    }
}
