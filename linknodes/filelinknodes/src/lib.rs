// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(conservative_impl_trait)]

extern crate futures;
extern crate futures_cpupool;
extern crate percent_encoding;

extern crate failure_ext as failure;
extern crate filekv;
extern crate futures_ext;
extern crate linknodes;
extern crate mercurial_types;
extern crate storage_types;

use std::path::PathBuf;
use std::sync::Arc;

use futures::Future;
use futures_cpupool::CpuPool;

use failure::Result;
use filekv::FileKV;
use futures_ext::{BoxFuture, FutureExt};
use linknodes::{Error as LinknodeError, ErrorKind as LinknodeErrorKind, LinknodeData, Linknodes,
                OptionNodeHash};
use mercurial_types::{NodeHash, RepoPath};
use mercurial_types::hash::Sha1;

static PREFIX: &str = "linknode-";

/// A basic file-based persistent linknode store.
///
/// Linknodes are stored as files in the specified base directory.
pub struct FileLinknodes {
    kv: FileKV<LinknodeData>,
}

impl FileLinknodes {
    #[inline]
    pub fn open<P: Into<PathBuf>>(path: P) -> Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::open(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn open_with_pool<P: Into<PathBuf>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::open_with_pool(path, PREFIX, pool)?,
        })
    }

    #[inline]
    pub fn create<P: Into<PathBuf>>(path: P) -> Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::create(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn create_with_pool<P: Into<PathBuf>>(path: P, pool: Arc<CpuPool>) -> Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::create_with_pool(path, PREFIX, pool)?,
        })
    }

    pub fn get_data(
        &self,
        path: RepoPath,
        node: &NodeHash,
    ) -> impl Future<Item = LinknodeData, Error = LinknodeError> + Send {
        let node = *node;
        self.kv
            .get(hash(&path, &node).to_hex())
            .then(move |res| match res {
                Ok(Some((data, _version))) => Ok(data),
                Ok(None) => Err(LinknodeErrorKind::NotFound(path, node).into()),
                Err(err) => Err(err.context(LinknodeErrorKind::StorageError).into()),
            })
    }
}

fn hash(path: &RepoPath, node: &NodeHash) -> Sha1 {
    // compute the hash of path + null byte + node
    let mut buf = path.serialize();
    buf.push(0);
    buf.extend_from_slice(node.as_ref());
    buf.as_slice().into()
}

impl Linknodes for FileLinknodes {
    type Get = BoxFuture<NodeHash, LinknodeError>;
    type Effect = BoxFuture<(), LinknodeError>;

    fn add(&self, path: RepoPath, node: &NodeHash, linknode: &NodeHash) -> Self::Effect {
        let node = *node;
        let linknode = *linknode;
        let hash = hash(&path, &node).to_hex();
        let linknode_data = LinknodeData {
            path: path.clone(),
            node,
            linknode,
        };
        self.kv
            .set_new(
                hash,
                &linknode_data,
                Some(1.into()), // Set a fixed version so that the bytes on disk are deterministic
            )
            .then(move |res| {
                match res {
                    Ok(Some(_)) => {
                        // Versions are irrelevant as linknodes don't support replacement.
                        Ok(())
                    }
                    Ok(None) => Err(
                        LinknodeErrorKind::AlreadyExists {
                            path,
                            node,
                            old_linknode: OptionNodeHash(None),
                            new_linknode: linknode,
                        }.into(),
                    ),
                    Err(err) => Err(err.context(LinknodeErrorKind::StorageError).into()),
                }
            })
            .boxify()
    }

    fn get(&self, path: RepoPath, node: &NodeHash) -> Self::Get {
        self.get_data(path, node).map(|data| data.linknode).boxify()
    }
}
