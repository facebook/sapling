/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::btree_map::Entry as BTreeEntry;
use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use failure_ext::{bail_err, Error, ResultExt};
use futures::{stream, IntoFuture};
use futures_ext::{BoxFuture, FutureExt, StreamExt};

use context::CoreContext;
use mercurial_types::blobnode::HgParents;
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::{HgEntryId, HgFileNodeId, HgManifestId, HgNodeHash};
use mercurial_types::{
    FileBytes, FileType, HgBlob, HgEntry, HgManifest, MPath, MPathElement, RepoPath, Type,
};

use crate::errors::*;

pub type ContentFactory = Arc<dyn (Fn() -> Content) + Send + Sync>;

pub fn make_file<C: Into<Bytes>>(file_type: FileType, content: C) -> ContentFactory {
    let content: Bytes = content.into();

    Arc::new(move || {
        let content = FileBytes(content.clone());
        let stream = stream::once(Ok(content)).boxify();
        Content::new_file(file_type, stream)
    })
}

#[derive(Clone)]
pub struct MockManifest {
    entries: BTreeMap<MPathElement, MockEntry>,
}

impl MockManifest {
    /// Create an empty manifest.
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Build a root tree manifest from a map of paths to file types and contents.
    ///
    /// dir_hashes is used to assign directories hashes.
    pub fn from_path_map(
        path_map: BTreeMap<MPath, (FileType, Bytes, Option<HgNodeHash>)>,
        dir_hashes: BTreeMap<MPath, HgNodeHash>,
    ) -> Result<Self> {
        // Stack of directory names and entry lists currently being built
        let mut wip: Vec<(Option<MPath>, _)> = vec![(None, BTreeMap::new())];

        for (path, (file_type, content, hash)) in path_map {
            // common_idx is the index of the last component that is common with this path.
            let common_idx = {
                let last_path = wip
                    .last()
                    .expect("wip should have at least 1 element")
                    .0
                    .as_ref();
                path.common_components(MPath::iter_opt(last_path))
            };
            // If files "foo" and "foo/bar" show up together in the same manifest, it's broken.
            // (But note that sort order means that this shouldn't happen anyway.)
            assert!(
                common_idx < path.num_components(),
                "a file cannot have the same name as a directory"
            );

            // Pop directories from the wip stack that are now done.
            finalize_dirs(&mut wip, common_idx, &dir_hashes)?;

            // Push new elements to the stack for any new intermediate directories.
            for idx in (common_idx + 1)..path.num_components() {
                let dir = path
                    .take_prefix_components(idx)
                    .expect("idx is always less than path components");
                wip.push((dir, BTreeMap::new()));
            }

            let basename = path.basename().clone();

            let cf = make_file(file_type, content);
            let mut entry = MockEntry::new(RepoPath::FilePath(path), cf);
            entry.set_type(Type::File(file_type));
            if let Some(h) = hash {
                entry.set_hash(h);
            }
            wip.last_mut()
                .expect("wip should have at least 1 element")
                .1
                .insert(basename, entry);
        }

        // Wrap up any remaining directories in the stack.
        finalize_dirs(&mut wip, 0, &dir_hashes)?;
        assert_eq!(
            wip.len(),
            1,
            "wip should have exactly 1 element left but has {}",
            wip.len()
        );
        let (_, entries) = wip.swap_remove(0);
        Ok(MockManifest { entries })
    }

    /// A generic version of `from_path_map`.
    pub fn from_path_hashes<IP, ID, P, B>(paths: IP, dir_hashes: ID) -> Result<Self>
    where
        IP: IntoIterator<Item = (P, (FileType, B, HgNodeHash))>,
        ID: IntoIterator<Item = (P, HgNodeHash)>,
        P: AsRef<[u8]>,
        B: Into<Bytes>,
    {
        let result: Result<BTreeMap<_, _>> = paths
            .into_iter()
            .map(|(p, (ft, b, id))| Ok((MPath::new(p)?, (ft, b.into(), Some(id)))))
            .collect();
        let result = result
            .with_context(|| ErrorKind::InvalidPathMap("error converting to MPath".into()))?;

        let dir_hashes: Result<BTreeMap<_, _>> = dir_hashes
            .into_iter()
            .map(|(p, hash)| Ok((MPath::new(p)?, hash)))
            .collect();
        let dir_hashes = dir_hashes.with_context(|| {
            ErrorKind::InvalidDirectoryHashes("error converting to MPath".into())
        })?;
        Self::from_path_map(result, dir_hashes)
    }

    /// A generic version of `from_path_map` that doesn't accept hashes for entry IDs.
    pub fn from_paths<I, P, B>(paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = (P, (FileType, B))>,
        P: AsRef<[u8]>,
        B: Into<Bytes>,
    {
        let result: Result<BTreeMap<_, _>> = paths
            .into_iter()
            .map(|(p, (ft, b))| Ok((MPath::new(p)?, (ft, b.into(), None))))
            .collect();
        let result =
            result.with_context(|| ErrorKind::InvalidPathMap("error converting to MPath".into()));
        Self::from_path_map(result?, BTreeMap::new())
    }
}

/// Pop directories from the end of the stack until and including 1 element after
/// last_to_keep.
fn finalize_dirs(
    wip: &mut Vec<(Option<MPath>, BTreeMap<MPathElement, MockEntry>)>,
    last_to_keep: usize,
    dir_hashes: &BTreeMap<MPath, HgNodeHash>,
) -> Result<()> {
    for _ in (last_to_keep + 1)..wip.len() {
        let (dir, entries) = wip.pop().expect("wip should have at least 1 element");
        let dir = dir.expect("wip[0] should never be popped");
        let basename = dir.basename().clone();

        let dir_manifest = MockManifest { entries };
        let hash = dir_hashes.get(&dir).cloned();
        let mut dir_entry = MockEntry::from_manifest(RepoPath::DirectoryPath(dir), dir_manifest);
        if let Some(hash) = hash {
            dir_entry.set_hash(hash);
        }

        match wip
            .last_mut()
            .expect("wip should have at least 1 element")
            .1
            .entry(basename)
        {
            BTreeEntry::Vacant(v) => v.insert(dir_entry),
            BTreeEntry::Occupied(o) => {
                bail_err!(ErrorKind::InvalidPathMap(format!(
                    "directory {} already present as type {:?}",
                    dir_entry.path,
                    o.get().get_type()
                )));
            }
        };
    }
    Ok(())
}

impl HgManifest for MockManifest {
    fn lookup(&self, path: &MPathElement) -> Option<Box<dyn HgEntry + Sync>> {
        self.entries.get(path).map(|e| e.clone().boxed())
    }
    fn list(&self) -> Box<dyn Iterator<Item = Box<dyn HgEntry + Sync>> + Send> {
        Box::new(self.entries.clone().into_iter().map(|e| e.1.boxed()))
    }
}

pub struct MockEntry {
    path: RepoPath,
    name: Option<MPathElement>,
    content_factory: ContentFactory,
    ty: Option<Type>,
    hash: Option<HgNodeHash>,
}

impl Clone for MockEntry {
    fn clone(&self) -> Self {
        MockEntry {
            path: self.path.clone(),
            name: self.name.clone(),
            content_factory: self.content_factory.clone(),
            ty: self.ty.clone(),
            hash: self.hash.clone(),
        }
    }
}

impl MockEntry {
    pub fn new(path: RepoPath, content_factory: ContentFactory) -> Self {
        let name = match path.clone() {
            RepoPath::RootPath => None,
            RepoPath::FilePath(path) | RepoPath::DirectoryPath(path) => {
                path.clone().into_iter().next_back()
            }
        };
        MockEntry {
            path,
            name,
            content_factory,
            ty: None,
            hash: None,
        }
    }

    #[inline]
    pub fn from_manifest(path: RepoPath, mf: MockManifest) -> Self {
        let cf = Arc::new(move || Content::Tree(Box::new(mf.clone())));
        let mut entry = MockEntry::new(path, cf);
        entry.set_type(Type::Tree);
        entry
    }

    pub fn set_type(&mut self, ty: Type) {
        self.ty = Some(ty);
    }

    pub fn set_hash(&mut self, hash: HgNodeHash) {
        self.hash = Some(hash);
    }
}

impl HgEntry for MockEntry {
    fn get_type(&self) -> Type {
        self.ty.expect("ty is not set!")
    }
    fn get_parents(&self, _ctx: CoreContext) -> BoxFuture<HgParents, Error> {
        unimplemented!();
    }
    fn get_raw_content(&self, _ctx: CoreContext) -> BoxFuture<HgBlob, Error> {
        unimplemented!();
    }
    fn get_content(&self, _ctx: CoreContext) -> BoxFuture<Content, Error> {
        Ok((self.content_factory)()).into_future().boxify()
    }
    fn get_size(&self, _ctx: CoreContext) -> BoxFuture<Option<u64>, Error> {
        unimplemented!();
    }
    fn get_hash(&self) -> HgEntryId {
        match (self.ty, self.hash) {
            (Some(ty), Some(hash)) => match ty {
                Type::File(file_type) => HgEntryId::File(file_type, HgFileNodeId::new(hash)),
                Type::Tree => HgEntryId::Manifest(HgManifestId::new(hash)),
            },
            _ => panic!(
                "hash for entry (name: '{:?}', type: '{:?}') is not set!",
                self.name, self.ty
            ),
        }
    }
    fn get_name(&self) -> Option<&MPathElement> {
        self.name.as_ref()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use futures::{Future, Stream};
    use maplit::btreemap;

    #[fbinit::test]
    fn lookup(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let paths = btreemap! {
                "foo/bar1" => (FileType::Regular, "bar1"),
                "foo/bar2" => (FileType::Symlink, "bar2"),
                "foo/baz/quux1" => (FileType::Executable, "quux1"),
                "quux2" => (FileType::Regular, "quux2"),
            };
            let root_manifest = MockManifest::from_paths(paths).expect("manifest is valid");

            assert!(
                root_manifest
                    .lookup(&MPathElement::new(b"not-present".to_vec()).unwrap())
                    .is_none(),
                "entry not present, should be None"
            );
            let foo_entry = root_manifest
                .lookup(&MPathElement::new(b"foo".to_vec()).unwrap())
                .expect("foo should be present");
            let foo_content = foo_entry
                .get_content(ctx.clone())
                .wait()
                .expect("content fetch should work");
            let foo_manifest = match foo_content {
                Content::Tree(manifest) => manifest,
                other => panic!("expected Tree content, found {:?}", other),
            };

            let bar1_entry = foo_manifest
                .lookup(&MPathElement::new(b"bar1".to_vec()).unwrap())
                .expect("bar1 should be present");
            let bar1_content = bar1_entry
                .get_content(ctx.clone())
                .wait()
                .expect("content fetch should work");
            let bar1_stream = match bar1_content {
                Content::File(stream) => stream,
                other => panic!("expected File content, found {:?}", other),
            };
            let bar1_bytes = bar1_stream
                .concat2()
                .wait()
                .expect("content stream should work");
            assert_eq!(bar1_bytes.into_bytes().as_ref(), &b"bar1"[..]);

            let bar2_entry = foo_manifest
                .lookup(&MPathElement::new(b"bar2".to_vec()).unwrap())
                .expect("bar2 should be present");
            let bar2_content = bar2_entry
                .get_content(ctx.clone())
                .wait()
                .expect("content fetch should work");
            let bar2_stream = match bar2_content {
                Content::Symlink(stream) => stream,
                other => panic!("expected Symlink content, found {:?}", other),
            };
            let bar2_bytes = bar2_stream
                .concat2()
                .wait()
                .expect("content stream should work");
            assert_eq!(bar2_bytes.into_bytes().as_ref(), &b"bar2"[..]);
        })
    }
}
