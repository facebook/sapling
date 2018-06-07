// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Root manifest, tree nodes

use std::collections::BTreeMap;
use std::str;
use std::sync::Arc;

use futures::future::{Future, IntoFuture};
use futures::stream;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial_types::{Entry, FileType, MPathElement, Manifest, Type};
use mercurial_types::nodehash::{HgEntryId, HgManifestId, HgNodeHash, NULL_HASH};

use blobstore::Blobstore;

use errors::*;
use file::HgBlobEntry;
use utils::{get_node, EnvelopeBlob};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Details {
    entryid: HgEntryId,
    flag: Type,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ManifestContent {
    pub files: BTreeMap<MPathElement, Details>,
}

impl ManifestContent {
    pub fn new_empty() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    // Each manifest revision contains a list of the file revisions in each changeset, in the form:
    //
    // <filename>\0<hex file revision id>[<flags>]\n
    //
    // Source: mercurial/parsers.c:parse_manifest()
    //
    // NB: filenames are sequences of non-zero bytes, not strings
    fn parse_impl(data: &[u8]) -> Result<BTreeMap<MPathElement, Details>> {
        let mut files = BTreeMap::new();

        for line in data.split(|b| *b == b'\n') {
            if line.len() == 0 {
                break;
            }

            let (name, rest) = match find(line, &0) {
                None => bail_msg!("Malformed entry: no \\0"),
                Some(nil) => {
                    let (name, rest) = line.split_at(nil);
                    if let Some((_, hash)) = rest.split_first() {
                        (name, hash)
                    } else {
                        bail_msg!("Malformed entry: no hash");
                    }
                }
            };

            let path = MPathElement::new(name.to_vec()).context("invalid path in manifest")?;
            let details = Details::parse(rest)?;

            files.insert(path, details);
        }

        Ok(files)
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        Ok(Self {
            files: Self::parse_impl(data)?,
        })
    }
}

pub struct BlobManifest {
    blobstore: Arc<Blobstore>,
    content: ManifestContent,
}

impl BlobManifest {
    pub fn load(
        blobstore: &Arc<Blobstore>,
        manifestid: &HgManifestId,
    ) -> BoxFuture<Option<Self>, Error> {
        let nodehash = manifestid.clone().into_nodehash();
        if nodehash == NULL_HASH {
            Ok(Some(BlobManifest {
                blobstore: blobstore.clone(),
                content: ManifestContent::new_empty(),
            })).into_future()
                .boxify()
        } else {
            get_node(blobstore, nodehash)
                .and_then({
                    let blobstore = blobstore.clone();
                    move |nodeblob| {
                        let blobkey = format!("sha1-{}", nodeblob.blob.sha1());
                        blobstore.get(blobkey)
                    }
                })
                .and_then({
                    let blobstore = blobstore.clone();
                    move |got| match got {
                        None => Ok(None),
                        Some(blob) => Ok(Some(Self::parse(blobstore, EnvelopeBlob::from(blob))?)),
                    }
                })
                .boxify()
        }
    }

    pub fn parse<D: AsRef<[u8]>>(blobstore: Arc<Blobstore>, data: D) -> Result<Self> {
        Self::create(blobstore, ManifestContent::parse(data.as_ref())?)
    }

    pub fn create(blobstore: Arc<Blobstore>, content: ManifestContent) -> Result<Self> {
        Ok(BlobManifest {
            blobstore: blobstore,
            content: content,
        })
    }
}

impl Manifest for BlobManifest {
    fn lookup(&self, path: &MPathElement) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        Ok(self.content.files.get(path).map({
            move |d| {
                HgBlobEntry::new(
                    self.blobstore.clone(),
                    path.clone(),
                    d.entryid().into_nodehash(),
                    d.flag(),
                ).boxed()
            }
        })).into_future()
            .boxify()
    }

    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        let entries = self.content.files.clone().into_iter().map({
            let blobstore = self.blobstore.clone();
            move |(path, d)| {
                HgBlobEntry::new(
                    blobstore.clone(),
                    path,
                    d.entryid().into_nodehash(),
                    d.flag(),
                ).boxed()
            }
        });
        stream::iter_ok(entries).boxify()
    }
}

impl Details {
    fn parse(data: &[u8]) -> Result<Details> {
        ensure_msg!(data.len() >= 40, "hash too small: {:?}", data);

        let (hash, flags) = data.split_at(40);
        let hash = str::from_utf8(hash)
            .map_err(|err| Error::from(err))
            .and_then(|hash| hash.parse::<HgNodeHash>())
            .with_context(|_| format!("malformed hash: {:?}", hash))?;
        let entryid = HgEntryId::new(hash);

        ensure_msg!(flags.len() <= 1, "More than 1 flag: {:?}", flags);

        let flag = if flags.len() == 0 {
            Type::File(FileType::Regular)
        } else {
            match flags[0] {
                b'l' => Type::File(FileType::Symlink),
                b'x' => Type::File(FileType::Executable),
                b't' => Type::Tree,
                unk => bail_msg!("Unknown flag {}", unk),
            }
        };

        Ok(Details {
            entryid: entryid,
            flag: flag,
        })
    }

    pub fn entryid(&self) -> &HgEntryId {
        &self.entryid
    }

    pub fn flag(&self) -> Type {
        self.flag
    }
}

fn find<T>(haystack: &[T], needle: &T) -> Option<usize>
where
    T: PartialEq,
{
    haystack.iter().position(|e| e == needle)
}
