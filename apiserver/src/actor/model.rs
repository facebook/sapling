// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// This file defines all types can be serialized into JSON

use std::convert::TryFrom;
use std::str;

use chrono::{DateTime, FixedOffset};
use failure::{err_msg, Error};

use blobrepo::HgBlobChangeset;
use context::CoreContext;
use futures::prelude::*;
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use mercurial_types::{Changeset as HgChangeset, Entry as HgEntry, Type};
use mercurial_types::hash::Sha1;
use mercurial_types::manifest::Content;

#[derive(Serialize)]
pub enum FileType {
    #[serde(rename = "file")] File,
    #[serde(rename = "tree")] Tree,
    #[serde(rename = "executable")] Executable,
    #[serde(rename = "symlink")] Symlink,
}

impl From<Type> for FileType {
    fn from(ttype: Type) -> FileType {
        use mononoke_types::FileType as MononokeFileType;

        match ttype {
            Type::File(ttype) => match ttype {
                MononokeFileType::Regular => FileType::File,
                MononokeFileType::Executable => FileType::Executable,
                MononokeFileType::Symlink => FileType::Symlink,
            },
            Type::Tree => FileType::Tree,
        }
    }
}

#[derive(Serialize)]
pub struct Entry {
    name: String,
    #[serde(rename = "type")]
    ttype: FileType,
    hash: String,
}

impl TryFrom<Box<HgEntry + Sync>> for Entry {
    type Error = Error;

    fn try_from(entry: Box<HgEntry + Sync>) -> Result<Entry, Self::Error> {
        let name = entry
            .get_name()
            .map(|name| name.to_bytes())
            .unwrap_or_else(|| Vec::new());
        let name = String::from_utf8(name)?;
        let ttype = entry.get_type().into();
        let hash = entry.get_hash().to_string();

        Ok(Entry { name, ttype, hash })
    }
}

#[derive(Serialize)]
pub struct EntryWithSizeAndContentHash {
    name: String,
    #[serde(rename = "type")]
    ttype: FileType,
    hash: String,
    size: Option<usize>,
    content_sha1: Option<String>,
}

impl EntryWithSizeAndContentHash {
    pub fn materialize_future(
        ctx: CoreContext,
        entry: Box<HgEntry + Sync>,
    ) -> BoxFuture<Self, Error> {
        let name = try_boxfuture!(
            entry
                .get_name()
                .map(|name| name.to_bytes())
                .ok_or_else(|| err_msg("HgEntry has no name!?"))
        );
        // FIXME: json cannot represent non-UTF8 file names
        let name = try_boxfuture!(String::from_utf8(name));
        let ttype = entry.get_type().into();
        let hash = entry.get_hash().to_string();

        spawn_future(entry.get_content(ctx).and_then(move |content| {
            let size = match &content {
                Content::File(contents)
                | Content::Executable(contents)
                | Content::Symlink(contents) => Some(contents.size()),
                Content::Tree(manifest) => Some(manifest.list().count()),
            };
            Ok(EntryWithSizeAndContentHash {
                name,
                ttype,
                hash,
                size,
                content_sha1: match content {
                    Content::File(contents)
                    | Content::Executable(contents)
                    | Content::Symlink(contents) => {
                        let sha1 = Sha1::from(contents.as_bytes().as_ref());
                        Some(sha1.to_hex().to_string())
                    }
                    Content::Tree(_) => None,
                },
            })
        })).boxify()
    }
}

#[derive(Serialize)]
pub struct Changeset {
    manifest: String,
    comment: String,
    date: DateTime<FixedOffset>,
    author: String,
}

impl TryFrom<HgBlobChangeset> for Changeset {
    type Error = str::Utf8Error;

    fn try_from(changeset: HgBlobChangeset) -> Result<Changeset, Self::Error> {
        let manifest = changeset.manifestid().to_string();
        let comment = str::from_utf8(changeset.comments())?.to_string();
        let date = changeset.time().into_chrono();
        let author = str::from_utf8(changeset.user())?.to_string();

        Ok(Changeset {
            manifest,
            comment,
            date,
            author,
        })
    }
}
