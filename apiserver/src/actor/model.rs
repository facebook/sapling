// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// This file defines all types can be serialized into JSON

use std::{
    collections::BTreeMap,
    convert::{Into, TryFrom, TryInto},
    str,
};

use abomonation_derive::Abomonation;
use chrono::{DateTime, FixedOffset};
use cloned::cloned;
use failure::{err_msg, Error};
use serde_derive::{Deserialize, Serialize};

use apiserver_thrift::types::{
    MononokeChangeset, MononokeFile, MononokeFileType, MononokeNodeHash, MononokeTreeHash,
};
use blobrepo::HgBlobChangeset;
use context::CoreContext;
use futures::prelude::*;
use futures_ext::{spawn_future, try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::hash::Sha1;
use mercurial_types::manifest::Content;
use mercurial_types::{Changeset as HgChangeset, Entry as HgEntry, Type};
use mononoke_types::RepositoryId;

use crate::cache::CacheManager;

#[derive(Abomonation, Clone, Serialize, Deserialize)]
pub enum FileType {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "tree")]
    Tree,
    #[serde(rename = "executable")]
    Executable,
    #[serde(rename = "symlink")]
    Symlink,
}

impl From<Type> for FileType {
    fn from(r#type: Type) -> FileType {
        use mononoke_types::FileType as MononokeFileType;

        match r#type {
            Type::File(r#type) => match r#type {
                MononokeFileType::Regular => FileType::File,
                MononokeFileType::Executable => FileType::Executable,
                MononokeFileType::Symlink => FileType::Symlink,
            },
            Type::Tree => FileType::Tree,
        }
    }
}

impl From<FileType> for MononokeFileType {
    fn from(file_type: FileType) -> Self {
        match file_type {
            FileType::File => MononokeFileType::FILE,
            FileType::Tree => MononokeFileType::TREE,
            FileType::Executable => MononokeFileType::EXECUTABLE,
            FileType::Symlink => MononokeFileType::SYMLINK,
        }
    }
}

impl From<Entry> for MononokeFile {
    fn from(entry: Entry) -> Self {
        Self {
            name: entry.name,
            file_type: entry.r#type.into(),
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Entry {
    name: String,
    r#type: FileType,
    hash: String,
}

impl TryFrom<Box<dyn HgEntry + Sync>> for Entry {
    type Error = Error;

    fn try_from(entry: Box<dyn HgEntry + Sync>) -> Result<Entry, Self::Error> {
        let name = entry
            .get_name()
            .map(|name| Vec::from(name.as_ref()))
            .unwrap_or_else(|| Vec::new());
        let name = String::from_utf8(name)?;
        let r#type = entry.get_type().into();
        let hash = entry.get_hash().to_string();

        Ok(Entry { name, r#type, hash })
    }
}

#[derive(Abomonation, Clone, Serialize, Deserialize)]
pub struct EntryWithSizeAndContentHash {
    name: String,
    r#type: FileType,
    hash: String,
    size: Option<u64>,
    content_sha1: Option<String>,
}

impl EntryWithSizeAndContentHash {
    fn get_cache_key(repoid: RepositoryId, hash: &str) -> String {
        format!("repo{}:{}", repoid.id(), hash)
    }

    pub fn materialize_future(
        ctx: CoreContext,
        repoid: RepositoryId,
        entry: Box<dyn HgEntry + Sync>,
        cache: Option<CacheManager>,
    ) -> BoxFuture<Self, Error> {
        let name = try_boxfuture!(entry
            .get_name()
            .map(|name| name.to_bytes())
            .ok_or_else(|| err_msg("HgEntry has no name!?")));
        // FIXME: json cannot represent non-UTF8 file names
        let name = try_boxfuture!(String::from_utf8(Vec::from(name.as_ref())));
        let r#type: FileType = entry.get_type().into();
        let hash = entry.get_hash().to_hex();

        let cache_key = Self::get_cache_key(repoid, hash.as_str());

        // this future computes SHA1 based on content
        let future = spawn_future(entry.get_content(ctx).and_then({
            cloned!(name, r#type, hash);
            move |content| {
                let inner = match content {
                    Content::File(stream)
                    | Content::Executable(stream)
                    | Content::Symlink(stream) => {
                        // TODO (T47717165): Use a streaming implementation / get sha in filestore
                        stream
                            .concat2()
                            .map(|file_bytes| {
                                let bytes = file_bytes.into_bytes();

                                // NOTE: This will only panic with a buffer whose length doesn't
                                // fit in 64 bits. We don't care to support this.
                                let size: u64 = bytes.len().try_into().unwrap();

                                let sha1 = Sha1::from(bytes.as_ref());
                                let sha1 = sha1.to_hex().to_string();

                                (Some(size), Some(sha1))
                            })
                            .left_future()
                    }
                    Content::Tree(manifest) => {
                        let size = manifest.list().count() as u64;
                        Ok((Some(size), None)).into_future().right_future()
                    }
                };

                inner.map(move |(size, content_sha1)| EntryWithSizeAndContentHash {
                    name,
                    r#type,
                    hash: hash.to_string(),
                    size,
                    content_sha1,
                })
            }
        }));

        if let Some(cache) = cache {
            cache
                .get_or_fill(cache_key, future.map(|entry| Some(entry)).from_err())
                .from_err()
                .and_then(move |entry| entry.ok_or(err_msg(format!("Entry {} not found", hash))))
                .map(|entry| EntryWithSizeAndContentHash {
                    name,
                    r#type,
                    ..entry
                })
                .boxify()
        } else {
            future.boxify()
        }
    }
}

impl From<EntryWithSizeAndContentHash> for MononokeFile {
    fn from(entry: EntryWithSizeAndContentHash) -> Self {
        Self {
            name: entry.name,
            file_type: entry.r#type.into(),
            hash: MononokeNodeHash { hash: entry.hash },
            size: entry.size.map(|size| size as i64),
            content_sha1: entry.content_sha1,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Changeset {
    commit_hash: String,
    manifest: String,
    comment: String,
    date: DateTime<FixedOffset>,
    author: String,
    parents: Vec<String>,
    extra: BTreeMap<String, Vec<u8>>,
}

impl TryFrom<HgBlobChangeset> for Changeset {
    type Error = str::Utf8Error;

    fn try_from(changeset: HgBlobChangeset) -> Result<Changeset, Self::Error> {
        let commit_hash = changeset.get_changeset_id().to_hex().to_string();
        let manifest = changeset.manifestid().to_string();
        let comment = str::from_utf8(changeset.comments())?.to_string();
        let date = changeset.time().into_chrono();
        let author = str::from_utf8(changeset.user())?.to_string();
        let parents: Vec<_> = vec![changeset.p1(), changeset.p2()]
            .into_iter()
            .flat_map(|p| p.map(|p| p.to_hex().to_string()))
            .collect();

        let extra = changeset
            .extra()
            .iter()
            .map(|(v1, v2)| (String::from_utf8_lossy(v1).into_owned(), v2.to_vec()))
            .collect();

        Ok(Changeset {
            commit_hash,
            manifest,
            comment,
            date,
            author,
            parents,
            extra,
        })
    }
}

impl From<Changeset> for MononokeChangeset {
    fn from(changeset: Changeset) -> Self {
        Self {
            commit_hash: changeset.commit_hash,
            message: changeset.comment,
            date: changeset.date.timestamp(),
            author: changeset.author,
            parents: changeset.parents,
            extra: changeset.extra,
            manifest: MononokeTreeHash {
                hash: changeset.manifest,
            },
        }
    }
}
