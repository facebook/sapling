// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry as BTreeEntry;
use std::io::{Cursor, Read};
use std::sync::Arc;

use bytes::Bytes;
use csv::Reader;
use failure::{Error, ResultExt};
use futures::{stream, IntoFuture};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial_types::{Blob, Entry, FileType, MPath, MPathElement, Manifest, RepoPath, Type};
use mercurial_types::blobnode::Parents;
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::EntryId;

use errors::*;

pub type ContentFactory = Arc<Fn() -> Content + Send + Sync>;

pub fn make_file<C: Into<Bytes>>(content: C) -> ContentFactory {
    let content = content.into();
    Arc::new(move || Content::File(Blob::Dirty(content.clone())))
}

#[derive(Clone)]
pub struct MockManifest {
    entries: BTreeMap<MPathElement, MockEntry>,
}

impl MockManifest {
    /// Build a root tree manifest from a description.
    ///
    /// A description is a CSV file with three fields (path, type and content).
    pub fn from_description<R: Read>(desc: R) -> Result<Self> {
        let mut reader = Reader::from_reader(desc).has_headers(false);
        let result: Result<BTreeMap<_, _>> = reader
            .byte_records()
            .map(|record| {
                let (path, file_type, content) = parse_record(record?)?;
                Ok((path, (file_type, content)))
            })
            .collect();
        let path_map = result?;
        Self::from_path_map(path_map)
    }

    /// Build a root tree manifest from a description string.
    pub fn from_description_string<P: AsRef<[u8]>>(desc: P) -> Result<Self> {
        Self::from_description(Cursor::new(desc))
    }

    /// Build a root tree manifest from a map of paths to file types and contents.
    pub fn from_path_map(path_map: BTreeMap<MPath, (FileType, Bytes)>) -> Result<Self> {
        // Stack of directory names and entry lists currently being built
        let mut wip: Vec<(Option<MPath>, _)> = vec![(None, BTreeMap::new())];

        for (path, (file_type, content)) in path_map {
            // common_idx is the index of the last component that is common with this path.
            let common_idx = {
                let last_path = wip.last()
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
            finalize_dirs(&mut wip, common_idx)?;

            // Push new elements to the stack for any new intermediate directories.
            for idx in (common_idx + 1)..path.num_components() {
                let dir = path.take_prefix_components(idx)
                    .expect("idx is always less than path components");
                wip.push((dir, BTreeMap::new()));
            }

            let basename = path.basename().clone();

            let cf = make_file(content);
            let mut entry = MockEntry::new(RepoPath::FilePath(path), cf);
            entry.set_type(Type::File(file_type));
            wip.last_mut()
                .expect("wip should have at least 1 element")
                .1
                .insert(basename, entry);
        }

        // Wrap up any remaining directories in the stack.
        finalize_dirs(&mut wip, 0)?;
        assert_eq!(
            wip.len(),
            1,
            "wip should have exactly 1 element left but has {}",
            wip.len()
        );
        let entries = wip.into_iter()
            .next()
            .expect("wip should have 1 element left")
            .1;
        Ok(MockManifest { entries })
    }

    /// A generic version of `from_path_map`.
    pub fn from_paths<I, P, B>(paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = (P, (FileType, B))>,
        P: AsRef<[u8]>,
        B: Into<Bytes>,
    {
        let result: Result<BTreeMap<_, _>> = paths
            .into_iter()
            .map(|(p, (ft, b))| Ok((MPath::new(p)?, (ft, b.into()))))
            .collect();
        let result =
            result.with_context(|_| ErrorKind::InvalidPathMap("error converting to MPath".into()));
        Self::from_path_map(result?)
    }
}

fn parse_record(mut record: Vec<Vec<u8>>) -> Result<(MPath, FileType, Bytes)> {
    if record.len() != 3 {
        bail_err!(ErrorKind::InvalidManifestDescription(format!(
            "expected CSV record to have 3 entries, found {}",
            record.len()
        )));
    }
    let path = MPath::new(&record[0])
        .with_context(|_| ErrorKind::InvalidManifestDescription("invalid path".into()))?;
    let file_type = match record[1].as_slice() {
        b"r" => FileType::Regular,
        b"x" => FileType::Executable,
        b"l" => FileType::Symlink,
        other => bail_err!(ErrorKind::InvalidManifestDescription(format!(
            "expected file type to be 'r', 'x' or 'l', found {:?}",
            String::from_utf8_lossy(other)
        ))),
    };
    let content = record.remove(2).into();
    Ok((path, file_type, content))
}

/// Pop directories from the end of the stack until and including 1 element after
/// last_to_keep.
fn finalize_dirs(
    wip: &mut Vec<(Option<MPath>, BTreeMap<MPathElement, MockEntry>)>,
    last_to_keep: usize,
) -> Result<()> {
    for _ in (last_to_keep + 1)..wip.len() {
        let (dir, entries) = wip.pop().expect("wip should have at least 1 element");
        let dir = dir.expect("wip[0] should never be popped");
        let basename = dir.basename().clone();

        let dir_manifest = MockManifest { entries };
        let cf = Arc::new(move || Content::Tree(Box::new(dir_manifest.clone())));
        let mut dir_entry = MockEntry::new(RepoPath::DirectoryPath(dir), cf);
        dir_entry.set_type(Type::Tree);

        match wip.last_mut()
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

impl Manifest for MockManifest {
    fn lookup(&self, _path: &MPathElement) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        unimplemented!();
    }
    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        stream::iter_ok(self.entries.clone().into_iter().map(|e| e.1.boxed())).boxify()
    }
}

pub struct MockEntry {
    path: RepoPath,
    name: Option<MPathElement>,
    content_factory: ContentFactory,
    ty: Option<Type>,
    hash: Option<EntryId>,
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

    pub fn set_type(&mut self, ty: Type) {
        self.ty = Some(ty);
    }

    pub fn set_hash(&mut self, hash: EntryId) {
        self.hash = Some(hash);
    }
}

impl Entry for MockEntry {
    fn get_type(&self) -> Type {
        self.ty.expect("ty is not set!")
    }
    fn get_parents(&self) -> BoxFuture<Parents, Error> {
        unimplemented!();
    }
    fn get_raw_content(&self) -> BoxFuture<Blob, Error> {
        unimplemented!();
    }
    fn get_content(&self) -> BoxFuture<Content, Error> {
        Ok((self.content_factory)()).into_future().boxify()
    }
    fn get_size(&self) -> BoxFuture<Option<usize>, Error> {
        unimplemented!();
    }
    fn get_hash(&self) -> &EntryId {
        match self.hash {
            Some(ref hash) => hash,
            None => panic!("hash is not set!"),
        }
    }
    fn get_name(&self) -> Option<&MPathElement> {
        self.name.as_ref()
    }
}
