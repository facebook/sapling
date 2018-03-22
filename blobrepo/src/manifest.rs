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
use futures::stream::{self, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial_types::{Entry, MPath, Manifest, Type};
use mercurial_types::nodehash::{EntryId, HgManifestId, NodeHash, NULL_HASH};

use blobstore::Blobstore;

use errors::*;
use file::BlobEntry;
use utils::get_node;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Details {
    entryid: EntryId,
    flag: Type,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ManifestContent {
    pub files: BTreeMap<MPath, Details>,
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
    fn parse_impl(data: &[u8], prefix: Option<&MPath>) -> Result<BTreeMap<MPath, Details>> {
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

            let path = if let Some(prefix) = prefix {
                prefix.join(&MPath::new(name).context("invalid path in manifest")?)
            } else {
                MPath::new(name).context("invalid path in manifest")?
            };
            let details = Details::parse(rest)?;

            // XXX check path > last entry in files
            files.insert(path, details);
        }

        Ok(files)
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        Ok(Self {
            files: Self::parse_impl(data, None)?,
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
                        Some(blob) => Ok(Some(Self::parse(blobstore, blob)?)),
                    }
                })
                .boxify()
        }
    }

    pub fn parse<D: AsRef<[u8]>>(blobstore: Arc<Blobstore>, data: D) -> Result<Self> {
        Ok(BlobManifest {
            blobstore: blobstore,
            content: ManifestContent::parse(data.as_ref())?,
        })
    }
}

impl Manifest for BlobManifest {
    fn lookup(&self, path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        // Path is a single MPathElement. In t25575327 we'll change the type.
        let name = path.clone().into_iter().next_back();

        let res = self.content.files.get(path).map({
            move |d| {
                BlobEntry::new(
                    self.blobstore.clone(),
                    name,
                    d.entryid().into_nodehash(),
                    d.flag(),
                )
            }
        });

        match res {
            Some(e_res) => e_res.map(|e| Some(e.boxed())).into_future().boxify(),
            None => Ok(None).into_future().boxify(),
        }
    }

    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        let entries = self.content
            .files
            .clone()
            .into_iter()
            .map({
                let blobstore = self.blobstore.clone();
                move |(path, d)| {
                    let name = path.clone().into_iter().next_back();
                    BlobEntry::new(
                        blobstore.clone(),
                        name,
                        d.entryid().into_nodehash(),
                        d.flag(),
                    )
                }
            })
            .map(|e_res| e_res.map(|e| e.boxed()));
        // TODO: (sid0) T23193289 replace with stream::iter_result once that becomes available
        stream::iter_ok(entries).and_then(|x| x).boxify()
    }
}

impl Details {
    fn parse(data: &[u8]) -> Result<Details> {
        ensure_msg!(data.len() >= 40, "hash too small: {:?}", data);

        let (hash, flags) = data.split_at(40);
        let hash = str::from_utf8(hash)
            .map_err(|err| Error::from(err))
            .and_then(|hash| hash.parse::<NodeHash>())
            .with_context(|_| format!("malformed hash: {:?}", hash))?;
        let entryid = EntryId::new(hash);

        ensure_msg!(flags.len() <= 1, "More than 1 flag: {:?}", flags);

        let flag = if flags.len() == 0 {
            Type::File
        } else {
            match flags[0] {
                b'l' => Type::Symlink,
                b'x' => Type::Executable,
                b't' => Type::Tree,
                unk => bail_msg!("Unknown flag {}", unk),
            }
        };

        Ok(Details {
            entryid: entryid,
            flag: flag,
        })
    }

    pub fn entryid(&self) -> &EntryId {
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
