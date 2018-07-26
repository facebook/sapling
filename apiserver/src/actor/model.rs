// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// This file defines all types can be serialized into JSON

use std::convert::TryFrom;

use failure::Error;

use mercurial_types::{Entry as HgEntry, Type};

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
