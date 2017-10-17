// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate futures;
extern crate futures_cpupool;
extern crate percent_encoding;

extern crate filekv;
extern crate futures_ext;
extern crate linknodes;
extern crate mercurial_types;
extern crate storage_types;

use std::path::Path;
use std::sync::Arc;

use futures::Future;
use futures_cpupool::CpuPool;

use filekv::FileKV;
use futures_ext::{BoxFuture, FutureExt};
use linknodes::{Error as LinknodeError, ErrorKind as LinknodeErrorKind, Linknodes, ResultExt};
use mercurial_types::{MPath, NodeHash};
use mercurial_types::hash::Sha1;

static PREFIX: &str = "linknode:";

/// A basic file-based persistent linknode store.
///
/// Linknodes are stored as files in the specified base directory.
pub struct FileLinknodes {
    kv: FileKV<NodeHash>,
}

impl FileLinknodes {
    #[inline]
    pub fn open<P: AsRef<Path>>(path: P) -> filekv::Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::open(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn open_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> filekv::Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::open_with_pool(path, PREFIX, pool)?,
        })
    }

    #[inline]
    pub fn create<P: AsRef<Path>>(path: P) -> filekv::Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::create(path, PREFIX)?,
        })
    }

    #[inline]
    pub fn create_with_pool<P: AsRef<Path>>(path: P, pool: Arc<CpuPool>) -> filekv::Result<Self> {
        Ok(FileLinknodes {
            kv: FileKV::create_with_pool(path, PREFIX, pool)?,
        })
    }
}

fn hash(path: &MPath, node: &NodeHash) -> Sha1 {
    // compute the hash of path + null byte + node
    let mut buf = path.to_vec();
    buf.push(0);
    buf.extend_from_slice(node.as_ref());
    buf.as_slice().into()
}

impl Linknodes for FileLinknodes {
    type Get = BoxFuture<NodeHash, LinknodeError>;
    type Effect = BoxFuture<(), LinknodeError>;

    fn add(&self, path: &MPath, node: &NodeHash, linknode: &NodeHash) -> Self::Effect {
        let path = path.clone();
        let node = *node;
        let linknode = *linknode;
        self.kv
            .set_new(hash(&path, &node).to_hex(), &linknode)
            .then(move |res| {
                match res {
                    Ok(Some(_)) => {
                        // Versions are irrelevant as linknodes don't support replacement.
                        Ok(())
                    }
                    Ok(None) => {
                        Err(LinknodeErrorKind::AlreadyExists(path, node, None, linknode).into())
                    }
                    Err(err) => Err(err)
                        .chain_err(|| LinknodeError::from_kind(LinknodeErrorKind::StorageError)),
                }
            })
            .boxify()
    }

    fn get(&self, path: &MPath, node: &NodeHash) -> Self::Get {
        let path = path.clone();
        let node = *node;
        self.kv
            .get(hash(&path, &node).to_hex())
            .then(move |res| match res {
                Ok(Some((nodehash, _version))) => Ok(nodehash),
                Ok(None) => Err(LinknodeErrorKind::NotFound(path, node).into()),
                Err(err) => {
                    Err(err).chain_err(|| LinknodeError::from_kind(LinknodeErrorKind::StorageError))
                }
            })
            .boxify()
    }
}
