/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use manifest::File;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use once_cell::sync::OnceCell;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use types::HgId;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::store;
use crate::store::InnerStore;

// Allows sending link between threads, but disallows general copying.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct Link {
    inner: Arc<LinkData>,
}

impl Deref for Link {
    type Target = LinkData;
    fn deref(&self) -> &LinkData {
        self.inner.as_ref()
    }
}

impl AsRef<LinkData> for Link {
    fn as_ref(&self) -> &LinkData {
        self.inner.as_ref()
    }
}

impl Clone for Link {
    fn clone(&self) -> Self {
        // Most code should not be aware of the fact that Link can be cloned as an Arc, so for the
        // default clone implementation do a deep clone. thread_copy() should be used for explicit
        // cases that need a shallow copy.
        Link::new(self.inner.as_ref().clone())
    }
}

/// `Link` describes the type of nodes that tree manifest operates on.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub enum LinkData {
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
pub use self::LinkData::*;

// TODO: Use Vec instead of BTreeMap
/// The inner structure of a durable link.
#[derive(Debug)]
pub struct DurableEntry {
    pub hgid: HgId,
    pub links: OnceCell<BTreeMap<PathComponentBuf, Link>>,
}

impl Link {
    pub fn new(link: LinkData) -> Self {
        Link {
            inner: Arc::new(link),
        }
    }

    pub fn durable(hgid: HgId) -> Link {
        Link::new(LinkData::Durable(Arc::new(DurableEntry::new(hgid))))
    }

    pub fn ephemeral() -> Link {
        Link::new(LinkData::Ephemeral(BTreeMap::new()))
    }

    pub fn leaf(metadata: FileMetadata) -> Link {
        Link::new(LinkData::Leaf(metadata))
    }

    pub fn mut_ephemeral_links(
        &mut self,
        store: &InnerStore,
        parent: &RepoPath,
    ) -> Result<&mut BTreeMap<PathComponentBuf, Link>> {
        self.as_mut_ref()?.mut_ephemeral_links(store, parent)
    }

    pub fn to_fs_node(&self) -> FsNodeMetadata {
        match self.as_ref() {
            Leaf(metadata) => FsNodeMetadata::File(*metadata),
            Ephemeral(_) => FsNodeMetadata::Directory(None),
            Durable(durable) => FsNodeMetadata::Directory(Some(durable.hgid)),
        }
    }

    /// Create a file record for a `Link`, failing if the link
    /// refers to a directory rather than a file.
    pub fn to_file(&self, path: RepoPathBuf) -> Option<File> {
        match self.as_ref() {
            Leaf(metadata) => Some(File::new(path, *metadata)),
            _ => None,
        }
    }

    pub fn matches(&self, matcher: &impl Matcher, path: &RepoPath) -> Result<bool> {
        match self.as_ref() {
            Leaf(_) => matcher.matches_file(path),
            Durable(_) | Ephemeral(_) => {
                Ok(matcher.matches_directory(path)? != DirectoryMatch::Nothing)
            }
        }
    }

    pub fn thread_copy(&self) -> Self {
        Link {
            inner: self.inner.clone(),
        }
    }

    pub fn as_mut_ref(&mut self) -> Result<&mut LinkData> {
        // This introduces an unsual mutability pattern where we allow mutations as long as there
        // is only one copy of the Link's Arc. That one copy will always be the parent directory.
        // In normal treemanifest operations, these Links are never shared beween trees (and in
        // fact the only way to copy the Arc is through the thread_copy function) so it will always
        // be the case that there is only one copy.
        //
        // The one exception, and the reason we use Arc at all, is during tree traversals. In that
        // case we pass copies of the Arc to other threads for parallel traversals. Therefore we
        // cannot use as_mut_ref() while traversing the tree.
        //
        // Normally we'd use RwLock so the compiler could enforce this, but having a RwLock on
        // every Link would be expensive and tree reads are a critical hotpath. Using Arc like this
        // gives us zero-cost reads and standard rust mutable-reference safety, as long as we don't
        // copy the Arc.

        Arc::get_mut(&mut self.inner).ok_or_else(|| {
            anyhow!("cannot mutate tree manifest link if there are multiple readers")
        })
    }
}

impl LinkData {
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
            };
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
        self.links.get_or_try_init(|| {
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
                        Link::leaf(FileMetadata::new(element.hgid, file_type))
                    }
                    store::Flag::Directory => Link::durable(element.hgid),
                };
                links.insert(element.component, link);
            }
            Ok(links)
        })
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
            (Some(a), Some(b)) => a == b,
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
pub struct DirLink {
    pub path: RepoPathBuf,
    pub link: Link,
}

impl DirLink {
    /// Create a directory record for a `Link`, failing if the link
    /// refers to a file rather than a directory.
    pub fn from_link(link: &Link, path: RepoPathBuf) -> Option<Self> {
        if let Leaf(_) = link.as_ref() {
            return None;
        }
        Some(DirLink {
            path,
            link: link.thread_copy(),
        })
    }

    /// Same as `from_link`, but set the directory's path to the empty
    /// path, making this method only useful for the root of the tree.
    pub fn from_root(link: &Link) -> Option<Self> {
        Self::from_link(link, RepoPathBuf::new())
    }

    pub fn hgid(&self) -> Option<HgId> {
        match self.link.as_ref() {
            Leaf(_) | Ephemeral(_) => None,
            Durable(entry) => Some(entry.hgid),
        }
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
    pub fn list(&self, store: &InnerStore) -> Result<(Vec<File>, Vec<DirLink>)> {
        let mut files = Vec::new();
        let mut dirs = Vec::new();

        let links = match self.link.as_ref() {
            Leaf(_) => panic!("programming error: directory cannot be a leaf node"),
            Ephemeral(ref links) => links,
            Durable(entry) => entry.materialize_links(store, &self.path)?,
        };

        for (name, link) in links {
            let mut path = self.path.clone();
            path.push(name.as_ref());
            match link.as_ref() {
                Leaf(_) => files.push(link.to_file(path).expect("leaf node must be a valid file")),
                Ephemeral(_) | Durable(_) => dirs.push(
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
        Some(Key::new(self.path.clone(), self.hgid().clone()?))
    }
}

impl Eq for DirLink {}

impl PartialEq for DirLink {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.hgid() == other.hgid()
    }
}

impl Ord for DirLink {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.path.cmp(&other.path) {
            Ordering::Equal => self.hgid().cmp(&other.hgid()),
            ord => ord,
        }
    }
}

impl PartialOrd for DirLink {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use manifest::testutil::*;
    use types::testutil::*;

    use super::*;
    use crate::testutil::*;

    #[test]
    fn test_file_from_link() {
        // Leaf link should result in a file.
        let meta = make_meta("a");
        let path = repo_path_buf("test/leaf");

        let leaf = Link::leaf(meta.clone());
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
            link: ephemeral,
        };
        assert_eq!(dir, expected);

        let hash = hgid("b");
        let durable = Link::durable(hash);
        let dir = DirLink::from_link(&durable, path.clone()).unwrap();
        let expected = DirLink {
            path: path.clone(),
            link: durable,
        };
        assert_eq!(dir, expected);

        // If the Link is actually a file, we should get None.
        let leaf = Link::leaf(meta.clone());
        let dir = DirLink::from_link(&leaf, path.clone());
        assert!(dir.is_none());
    }

    #[test]
    fn test_list_directory() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let tree = make_tree_manifest(store, &[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = DirLink::from_root(&tree.root).unwrap();
        let (files, dirs) = dir.list(&tree.store)?;

        let file_names = files.into_iter().map(|f| f.path).collect::<Vec<_>>();
        let dir_names = dirs.into_iter().map(|d| d.path).collect::<Vec<_>>();

        assert_eq!(file_names, vec![repo_path_buf("a"), repo_path_buf("c")]);
        assert_eq!(dir_names, vec![repo_path_buf("b"), repo_path_buf("d")]);

        Ok(())
    }
}
