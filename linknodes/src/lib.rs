// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate serde;
#[macro_use]
extern crate serde_derive;

extern crate mercurial_types;

use std::fmt;
use std::sync::Arc;

use futures::Future;

use mercurial_types::{NodeHash, RepoPath};

mod errors {
    use super::*;

    struct OptionNodeHash<'a>(&'a Option<NodeHash>);

    impl<'a> fmt::Display for OptionNodeHash<'a> {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            match *self.0 {
                Some(nodehash) => nodehash.fmt(fmt),
                None => write!(fmt, "(unknown)"),
            }
        }
    }

    error_chain! {
        errors {
            NotFound(path: RepoPath, node: NodeHash) {
                description("linknode not found")
                display("linknode not found for {}, node {}", path, node)
            }
            AlreadyExists(
                path: RepoPath,
                node: NodeHash,
                old_linknode: Option<NodeHash>,
                new_linknode: NodeHash
            ) {
                description("linknode already exists")
                display(
                    "linknode already exists for {}, node {} (linknodes: existing {}, new {})",
                    path,
                    node,
                    OptionNodeHash(old_linknode),
                    new_linknode
                )
            }
            StorageError {
                description("linknode storage error")
                display("linknode storage error")
            }
        }
    }
}

pub use errors::*;

/// Trait representing the interface to a linknodes store, which maps a path plus manifest or file
/// node hash to a changeset hash. At the moment this is a 1:1 mapping, but this will eventually
/// allow a 1:many mapping.
///
/// In principle, linknodes (especially 1:many) can be cached and regenerated. In practice,
/// Mercurial's storage and wire protocol is designed around storing linknodes as intrinsic data,
/// so Mononoke does the same.
pub trait Linknodes: Send + Sync + 'static {
    // Get will become a Stream once 1:many mappings are enabled.
    type Get: Future<Item = NodeHash, Error = Error> + Send + 'static;
    type Effect: Future<Item = (), Error = Error> + Send + 'static;

    fn add(&self, path: RepoPath, node: &NodeHash, linknode: &NodeHash) -> Self::Effect;
    fn get(&self, path: RepoPath, node: &NodeHash) -> Self::Get;
}

impl<L> Linknodes for Arc<L>
where
    L: Linknodes,
{
    type Get = L::Get;
    type Effect = L::Effect;

    #[inline]
    fn get(&self, path: RepoPath, node: &NodeHash) -> Self::Get {
        (**self).get(path, node)
    }

    #[inline]
    fn add(&self, path: RepoPath, node: &NodeHash, linknode: &NodeHash) -> Self::Effect {
        (**self).add(path, node, linknode)
    }
}

/// A struct representing all the data associated with a linknode. This definition is here so that
/// it can be shared across memory-based and file-based linknodes.
#[derive(Clone, Serialize, Deserialize)]
pub struct LinknodeData {
    pub path: RepoPath,
    pub node: NodeHash,
    pub linknode: NodeHash,
}
