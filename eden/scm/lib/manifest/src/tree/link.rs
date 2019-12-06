/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::BTreeMap, sync::Arc};

use anyhow::{bail, format_err, Context, Result};
use once_cell::sync::OnceCell;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{HgId, PathComponentBuf, RepoPath, RepoPathBuf};

use crate::tree::{store, store::InnerStore};
use crate::{File, FileMetadata, FsNode};

/// `Link` describes the type of nodes that tree manifest operates on.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub enum Link {
    /// `Leaf` nodes store FileMetadata. They are terminal nodes and don't have any other
    /// information.
    Leaf(FileMetadata),
    /// `Ephemeral` nodes are inner nodes that have not been committed to storage. They are only
    /// available in memory. They need to be persisted to be available in future. They are the
    /// mutable type of an inner node. They store the contents of a directory that has been
    /// modified.
    Ephemeral(BTreeMap<PathComponentBuf, Link>),
    /// `Durable` nodes are inner nodes that come from storage. Their contents can be
    /// shared between multiple instances of Tree. They are lazily evaluated. Their children
    /// list will be read from storage only when it is accessed.
    Durable(Arc<DurableEntry>),
}
pub use self::Link::*;

// TODO: Use Vec instead of BTreeMap
/// The inner structure of a durable link. Of note is that failures are cached "forever".
// The interesting question about this structure is what do we do when we have a failure when
// reading from storage?
// We can cache the failure or we don't cache it. Caching it is mostly fine if we had an error
// reading from local storage or when deserializing. It is not the best option if our storage
// is remote and we hit a network blip. On the other hand we would not want to always retry when
// there is a failure on remote storage, we'd want to have a least an exponential backoff on
// retries. Long story short is that caching the failure is a reasonable place to start from.
#[derive(Debug)]
pub struct DurableEntry {
    pub hgid: HgId,
    pub links: OnceCell<Result<BTreeMap<PathComponentBuf, Link>>>,
}

impl Link {
    pub fn durable(hgid: HgId) -> Link {
        Link::Durable(Arc::new(DurableEntry::new(hgid)))
    }

    #[cfg(test)]
    pub fn ephemeral() -> Link {
        Link::Ephemeral(BTreeMap::new())
    }

    pub fn mut_ephemeral_links(
        &mut self,
        store: &InnerStore,
        parent: &RepoPath,
    ) -> Result<&mut BTreeMap<PathComponentBuf, Link>> {
        loop {
            match self {
                Leaf(_) => bail!("Path {} is a file but a directory was expected.", parent),
                Ephemeral(ref mut links) => return Ok(links),
                Durable(ref entry) => {
                    let durable_links = entry.materialize_links(store, parent)?;
                    *self = Ephemeral(durable_links.clone());
                }
            }
        }
    }

    pub fn to_fs_node(&self) -> FsNode {
        match self {
            &Link::Leaf(metadata) => FsNode::File(metadata),
            Link::Ephemeral(_) => FsNode::Directory(None),
            Link::Durable(durable) => FsNode::Directory(Some(durable.hgid)),
        }
    }

    /// Create a file record for a `Link`, failing if the link
    /// refers to a directory rather than a file.
    pub fn to_file(&self, path: RepoPathBuf) -> Option<File> {
        match self {
            Leaf(metadata) => Some(File::new(path, *metadata)),
            _ => None,
        }
    }

    pub fn matches(&self, matcher: &impl Matcher, path: &RepoPath) -> bool {
        match self {
            Link::Leaf(_) => matcher.matches_file(path),
            Link::Durable(_) | Link::Ephemeral(_) => {
                matcher.matches_directory(path) != DirectoryMatch::Nothing
            }
        }
    }
}

impl DurableEntry {
    pub fn new(hgid: HgId) -> Self {
        DurableEntry {
            hgid,
            links: OnceCell::new(),
        }
    }

    pub fn materialize_links(
        &self,
        store: &InnerStore,
        path: &RepoPath,
    ) -> Result<&BTreeMap<PathComponentBuf, Link>> {
        // TODO: be smarter around how failures are handled when reading from the store
        // Currently this loses the stacktrace
        let result = self.links.get_or_init(|| {
            let entry = store
                .get_entry(path, self.hgid)
                .with_context(|| format!("failed fetching from store ({}, {})", path, self.hgid))?;
            let mut links = BTreeMap::new();
            for element_result in entry.elements() {
                let element = element_result.with_context(|| {
                    format!(
                        "failed to deserialize manifest entry {:?} for ({}, {})",
                        entry, path, self.hgid
                    )
                })?;
                let link = match element.flag {
                    store::Flag::File(file_type) => {
                        Leaf(FileMetadata::new(element.hgid, file_type))
                    }
                    store::Flag::Directory => Link::durable(element.hgid),
                };
                links.insert(element.component, link);
            }
            Ok(links)
        });
        result.as_ref().map_err(|e| format_err!("{:?}", e))
    }

    pub fn get_links(&self) -> Option<Result<&BTreeMap<PathComponentBuf, Link>>> {
        self.links
            .get()
            .as_ref()
            .map(|result| result.as_ref().map_err(|e| format_err!("{:?}", e)))
    }
}

// `PartialEq` can't be derived because `fallible::Error` does not implement `PartialEq`.
// It should also be noted that `self.links.get() != self.links.get()` can evaluate to true when
// `self.links` are being instantiated.
#[cfg(test)]
impl PartialEq for DurableEntry {
    fn eq(&self, other: &DurableEntry) -> bool {
        if self.hgid != other.hgid {
            return false;
        }
        match (self.links.get(), other.links.get()) {
            (None, None) => true,
            (Some(Ok(a)), Some(Ok(b))) => a == b,
            _ => false,
        }
    }
}
