/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

use anyhow::{bail, format_err, Context, Result};
use once_cell::sync::OnceCell;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{HgId, Key, PathComponentBuf, RepoPath, RepoPathBuf};

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

/// A directory (inner node) encountered during a tree traversal.
///
/// The directory may have a manifest hgid if it is unmodified from its
/// state on disk. If the directory has in-memory modifications that have not
/// been persisted to disk, it will not have an hgid.
#[derive(Clone, Debug)]
pub struct DirLink<'a> {
    pub path: RepoPathBuf,
    pub hgid: Option<HgId>,
    pub link: &'a Link,
}

impl<'a> DirLink<'a> {
    /// Create a directory record for a `Link`, failing if the link
    /// refers to a file rather than a directory.
    pub fn from_link(link: &'a Link, path: RepoPathBuf) -> Option<Self> {
        let hgid = match link {
            Link::Leaf(_) => return None,
            Link::Ephemeral(_) => None,
            Link::Durable(entry) => Some(entry.hgid),
        };
        Some(DirLink { path, hgid, link })
    }

    /// Same as `from_link`, but set the directory's path to the empty
    /// path, making this method only useful for the root of the tree.
    pub fn from_root(link: &'a Link) -> Option<Self> {
        Self::from_link(link, RepoPathBuf::new())
    }

    /// List the contents of this directory.
    ///
    /// Returns two sorted vectors of files and directories contained
    /// in this directory.
    ///
    /// This operation may perform I/O to load the tree entry from the store
    /// if it is not already in memory. Depending on the store implementation,
    /// this may involve an expensive network request if the required data is
    /// not available locally. As such, algorithms that require fast access to
    /// this data should take care to ensure that this content is present
    /// locally before calling this method.
    pub fn list(&self, store: &InnerStore) -> Result<(Vec<File>, Vec<DirLink<'a>>)> {
        let mut files = Vec::new();
        let mut dirs = Vec::new();

        let links = match &self.link {
            &Link::Leaf(_) => panic!("programming error: directory cannot be a leaf node"),
            &Link::Ephemeral(ref links) => links,
            &Link::Durable(entry) => entry.materialize_links(store, &self.path)?,
        };

        for (name, link) in links {
            let mut path = self.path.clone();
            path.push(name.as_ref());
            match link {
                Link::Leaf(_) => {
                    files.push(link.to_file(path).expect("leaf node must be a valid file"))
                }
                Link::Ephemeral(_) | Link::Durable(_) => dirs.push(
                    DirLink::from_link(link, path).expect("inner node must be a valid directory"),
                ),
            }
        }

        Ok((files, dirs))
    }

    /// Create a `Key` (path/hgid pair) corresponding to this directory. Keys are used
    /// by the Eden API to fetch data from the server, making this representation useful
    /// for interacting with Mercurial's data fetching code.
    pub fn key(&self) -> Option<Key> {
        Some(Key::new(self.path.clone(), self.hgid.clone()?))
    }
}

impl Eq for DirLink<'_> {}

impl PartialEq for DirLink<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.hgid == other.hgid
    }
}

impl Ord for DirLink<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.path.cmp(&other.path) {
            Ordering::Equal => self.hgid.cmp(&other.hgid),
            ord => ord,
        }
    }
}

impl PartialOrd for DirLink<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use types::testutil::*;

    use crate::tree::testutil::*;

    #[test]
    fn test_file_from_link() {
        // Leaf link should result in a file.
        let meta = make_meta("a");
        let path = repo_path_buf("test/leaf");

        let leaf = Link::Leaf(meta.clone());
        let file = leaf.to_file(path.clone()).unwrap();

        let expected = File {
            path: path.clone(),
            meta,
        };
        assert_eq!(file, expected);

        // Attempting to use a directory link should fail.
        let ephemeral = Link::ephemeral();
        let _file = ephemeral.to_file(path.clone());

        // Durable link should result in a directory.
        let durable = Link::durable(hgid("a"));
        let file = durable.to_file(path.clone());
        assert!(file.is_none());
    }

    #[test]
    fn test_directory_from_link() {
        let meta = make_meta("a");
        let path = repo_path_buf("test/leaf");

        let ephemeral = Link::ephemeral();
        let dir = DirLink::from_link(&ephemeral, path.clone()).unwrap();
        let expected = DirLink {
            path: path.clone(),
            hgid: None,
            link: &ephemeral,
        };
        assert_eq!(dir, expected);

        let hash = hgid("b");
        let durable = Link::durable(hash);
        let dir = DirLink::from_link(&durable, path.clone()).unwrap();
        let expected = DirLink {
            path: path.clone(),
            hgid: Some(hash),
            link: &ephemeral,
        };
        assert_eq!(dir, expected);

        // If the Link is actually a file, we should get None.
        let leaf = Link::Leaf(meta.clone());
        let dir = DirLink::from_link(&leaf, path.clone());
        assert!(dir.is_none());
    }

    #[test]
    fn test_list_directory() -> Result<()> {
        let tree = make_tree(&[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = DirLink::from_root(&tree.root).unwrap();
        let (files, dirs) = dir.list(&tree.store)?;

        let file_names = files.into_iter().map(|f| f.path).collect::<Vec<_>>();
        let dir_names = dirs.into_iter().map(|d| d.path).collect::<Vec<_>>();

        assert_eq!(file_names, vec![repo_path_buf("a"), repo_path_buf("c")]);
        assert_eq!(dir_names, vec![repo_path_buf("b"), repo_path_buf("d")]);

        Ok(())
    }
}
