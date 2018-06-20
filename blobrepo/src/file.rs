// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Plain files, symlinks

use futures::future::Future;
use futures_ext::{BoxFuture, FutureExt};

use mercurial::file;
use mercurial_types::{FileType, HgBlob, HgManifestId, HgNodeHash, HgParents, MPath, MPathElement};
use mercurial_types::manifest::{Content, Entry, Manifest, Type};
use mercurial_types::nodehash::HgEntryId;
use mononoke_types::FileContents;

use blobstore::Blobstore;

use errors::*;

use manifest::BlobManifest;

use repo::RepoBlobstore;
use utils::{get_node, RawNodeBlob};

#[derive(Clone)]
pub struct HgBlobEntry {
    blobstore: RepoBlobstore,
    name: Option<MPathElement>,
    id: HgEntryId,
    ty: Type,
}

impl PartialEq for HgBlobEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.id == other.id && self.ty == other.ty
    }
}

impl Eq for HgBlobEntry {}

pub fn fetch_raw_filenode_bytes(
    blobstore: &RepoBlobstore,
    nodeid: HgNodeHash,
) -> BoxFuture<HgBlob, Error> {
    get_node(blobstore, nodeid)
        .context("While fetching node for blob")
        .map_err(Error::from)
        .and_then({
            let blobstore = blobstore.clone();
            move |node| {
                let key = format!("sha1-{}", node.blob.sha1());
                let err_context = format!("While fetching content for blob key {}", key);

                blobstore
                    .get(key)
                    .and_then(move |blob| {
                        blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                    })
                    .map(HgBlob::from)
                    .context(err_context)
                    .from_err()
            }
        })
        .context(format!("While fetching raw bytes for {}", nodeid))
        .from_err()
        .boxify()
}

pub fn fetch_file_content_and_renames_from_blobstore(
    blobstore: &RepoBlobstore,
    nodeid: HgNodeHash,
) -> BoxFuture<(FileContents, Option<(MPath, HgNodeHash)>), Error> {
    get_node(blobstore, nodeid)
        .and_then({
            let blobstore = blobstore.clone();
            move |node| {
                let key = format!("sha1-{}", node.blob.sha1());
                let parents = node.parents;

                blobstore.get(key).and_then(move |blob| {
                    blob.ok_or(ErrorKind::ContentMissing(nodeid, node.blob).into())
                        .and_then(|blob| {
                            // XXX this is broken -- parents.get_nodes() will never return
                            // (None, Some(hash)), which is what BlobNode relies on to figure out
                            // whether a node is copied.
                            let (p1, p2) = parents.get_nodes();
                            let file = file::File::new(HgBlob::from(blob), p1, p2);

                            file.copied_from().map(|from| (file.file_contents(), from))
                        })
                })
            }
        })
        .boxify()
}

impl HgBlobEntry {
    pub fn new(blobstore: RepoBlobstore, name: MPathElement, nodeid: HgNodeHash, ty: Type) -> Self {
        Self {
            blobstore,
            name: Some(name),
            id: HgEntryId::new(nodeid),
            ty,
        }
    }

    pub fn new_root(blobstore: RepoBlobstore, manifestid: HgManifestId) -> Self {
        Self {
            blobstore,
            name: None,
            id: HgEntryId::new(manifestid.into_nodehash()),
            ty: Type::Tree,
        }
    }

    fn get_node(&self) -> BoxFuture<RawNodeBlob, Error> {
        get_node(&self.blobstore, self.id.into_nodehash())
    }

    fn get_raw_content_inner(&self) -> BoxFuture<HgBlob, Error> {
        fetch_raw_filenode_bytes(&self.blobstore, self.id.into_nodehash())
    }
}

impl Entry for HgBlobEntry {
    fn get_type(&self) -> Type {
        self.ty
    }

    fn get_parents(&self) -> BoxFuture<HgParents, Error> {
        self.get_node().map(|node| node.parents).boxify()
    }

    fn get_raw_content(&self) -> BoxFuture<HgBlob, Error> {
        self.get_raw_content_inner()
    }

    fn get_content(&self) -> BoxFuture<Content, Error> {
        let blobstore = self.blobstore.clone();
        self.get_raw_content()
            .and_then({
                let ty = self.ty;
                move |blob| {
                    // Mercurial file blob can have metadata, but tree manifest can't
                    let res = match ty {
                        Type::Tree => Content::Tree(
                            BlobManifest::parse(
                                blobstore,
                                blob.as_slice().expect("HgBlob should always have data"),
                            )?.boxed(),
                        ),
                        Type::File(ft) => {
                            let f = file::File::data_only(blob);
                            let contents = f.file_contents();
                            match ft {
                                FileType::Regular => Content::File(contents),
                                FileType::Executable => Content::Executable(contents),
                                FileType::Symlink => Content::Symlink(contents),
                            }
                        }
                    };
                    Ok(res)
                }
            })
            .context(format!(
                "While HgBlobEntry::get_content for id {}, name {:?}, type {:?}",
                self.id, self.name, self.ty
            ))
            .from_err()
            .boxify()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Error> {
        self.get_content()
            .and_then(|content| match content {
                Content::File(data) | Content::Executable(data) | Content::Symlink(data) => {
                    Ok(Some(data.size()))
                }
                Content::Tree(_) => Ok(None),
            })
            .boxify()
    }

    fn get_hash(&self) -> &HgEntryId {
        &self.id
    }

    fn get_name(&self) -> Option<&MPathElement> {
        self.name.as_ref()
    }
}
