// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial::manifest::Details;

use file::GQLFile;
use manifest::GQLPath;
use node::GQLNodeId;
use repo::RepoCtx;
use symlink::GQLSymlink;

pub trait ManifestObj {
    fn path(&self) -> &GQLPath;
    fn nodeid(&self) -> &GQLNodeId;
}

pub enum GQLManifestObj {
    File(GQLFile),
    Symlink(GQLSymlink),
}

impl GQLManifestObj {
    pub fn new(path: &GQLPath, details: &Details) -> Self {
        if details.is_file() {
            GQLManifestObj::File(GQLFile::new(path, details.nodeid()))
        } else if details.is_symlink() {
            GQLManifestObj::Symlink(GQLSymlink::new(path, details.nodeid()))
        } else {
            // treenodes, something else?
            unimplemented!()
        }
    }
}

impl From<GQLFile> for GQLManifestObj {
    fn from(file: GQLFile) -> Self {
        GQLManifestObj::File(file)
    }
}

impl From<GQLSymlink> for GQLManifestObj {
    fn from(symlink: GQLSymlink) -> Self {
        GQLManifestObj::Symlink(symlink)
    }
}

impl ManifestObj for GQLManifestObj {
    fn path(&self) -> &GQLPath {
        match self {
            &GQLManifestObj::File(ref file) => file.path(),
            &GQLManifestObj::Symlink(ref symlink) => symlink.path(),
        }
    }

    fn nodeid(&self) -> &GQLNodeId {
        match self {
            &GQLManifestObj::File(ref file) => file.nodeid(),
            &GQLManifestObj::Symlink(ref symlink) => symlink.nodeid(),
        }
    }
}

graphql_interface!(GQLManifestObj: RepoCtx as "ManifestObj" |&self| {
    description: "An object in the manifest"

    field path() -> &GQLPath {
        self.path()
    }

    field id() -> &GQLNodeId {
        self.nodeid()
    }

    instance_resolvers: |&_| {
        &GQLFile => {
            match self {
                &GQLManifestObj::File(ref file) => Some(file),
                _ => None,
            }
        },
        &GQLSymlink => {
            match self {
                &GQLManifestObj::Symlink(ref symlink) => Some(symlink),
                _ => None,
            }
        },
    }
});
