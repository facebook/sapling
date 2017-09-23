// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::str;

use juniper::FieldResult;

use mercurial::symlink::Symlink;
use mercurial_types::{MPath, NodeHash};

use manifest::GQLPath;
use manifestobj::{GQLManifestObj, ManifestObj};
use node::GQLNodeId;
use repo::RepoCtx;

pub struct GQLSymlink(GQLPath, GQLNodeId);

impl GQLSymlink {
    pub fn new(path: &MPath, id: &NodeHash) -> Self {
        GQLSymlink(GQLPath::from(path), GQLNodeId::from(id))
    }
}

impl ManifestObj for GQLSymlink {
    fn path(&self) -> &GQLPath {
        &self.0
    }

    fn nodeid(&self) -> &GQLNodeId {
        &self.1
    }
}

graphql_object!(GQLSymlink: RepoCtx as "Symlink" |&self| {
    description: "A file in the repo"
    interfaces: [ &GQLManifestObj ]

    field path() -> &GQLPath {
        &self.0
    }

    field id() -> &GQLNodeId {
        &self.1
    }

    field target(&executor) -> FieldResult<GQLPath> {
        let mut repo = executor.context().repo();
        let mut filelog = repo.get_file_revlog(self.path())
            .map_err(|err| format!("open {:?}: {:?}", self.path(), err))?;
        let node = filelog.get_rev_by_nodeid(self.nodeid())?;
        let symlink = Symlink::new(node);

        symlink.path()?
            .ok_or("no path content".into())
            .map(GQLPath::new)
    }

    field size(&executor) -> FieldResult<i64> {
        let mut repo = executor.context().repo();
        let mut entry = repo.get_file_revlog(self.path())
            .map_err(|err| format!("open {:?}: {:?}", self.path(), err))
            .and_then(|revlog| revlog.get_entry_by_nodeid(self.nodeid()).map_err(From::from))?;

        entry.len.map(|sz| sz as i64).ok_or("File has no size".into())
    }

    field parents(&executor) -> FieldResult<Vec<GQLSymlink>>
            as "get changeset's parents" {
        let mut repo = executor.context().repo();
        let mut filelog = repo.get_file_revlog(self.path())
            .map_err(|err| format!("open {:?}: {:?}", self.path(), err))?;

        filelog.get_rev_by_nodeid(self.nodeid())
            .map(|node| node.parents().into_iter()
                .map(|p| GQLSymlink::new(self.path(), &p))
                .collect())
            .map_err(From::from)
    }
});
