// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::str;

use juniper::FieldResult;

use mercurial::file::File;
use mercurial_types::{NodeHash, Path};

use repo::RepoCtx;
use node::GQLNodeId;
use manifest::GQLPath;
use manifestobj::{GQLManifestObj, ManifestObj};

pub struct GQLFile(GQLPath, GQLNodeId);

impl GQLFile {
    pub fn new(path: &Path, id: &NodeHash) -> Self {
        GQLFile(GQLPath::from(path), GQLNodeId::from(id))
    }
}

impl ManifestObj for GQLFile {
    fn path(&self) -> &GQLPath {
        &self.0
    }

    fn nodeid(&self) -> &GQLNodeId {
        &self.1
    }
}

graphql_object!(GQLFile: RepoCtx as "File" |&self| {
    description: "A file in the repo"
    interfaces: [ &GQLManifestObj ]

    field path() -> &GQLPath {
        self.path()
    }

    field id() -> &GQLNodeId {
        self.nodeid()
    }

    // XXX can't return raw binary data, so coerce to text
    field contents(&executor) -> FieldResult<String> {
        let mut repo = executor.context().repo();
        let mut filelog = repo.get_file_revlog(self.path())
            .map_err(|err| format!("open {:?}: {:?}", self.path(), err))?;
        let file = File::new(filelog.get_rev_by_nodeid(self.nodeid())?);

        file.content().ok_or("no file content".into())
            .map(|s| String::from_utf8_lossy(s).into_owned())
    }

    field size(&executor) -> FieldResult<i64> {
        let mut repo = executor.context().repo();
        let mut filelog = repo.get_file_revlog(self.path())
            .map_err(|err| format!("open {:?}: {:?}", self.path(), err))?;

        // Try getting the size without loading/constructing the file content, but
        // if that fails (likely because it has rename metadata or multiple parents),
        // then actually load the real data and try again.
        let size =
            match File::new(filelog.get_node_by_nodeid(self.nodeid(), false)?).size() {
                Some(sz) => sz,
                None => File::new(filelog.get_node_by_nodeid(self.nodeid(), true)?).size()
                    .expect("File is missing data despite loading it?"),
            };

        Ok(size as i64)
    }

    field parents(&executor) -> FieldResult<Vec<GQLFile>>
            as "get changeset's parents" {
        let mut repo = executor.context().repo();
        let mut filelog = repo.get_file_revlog(self.path())
            .map_err(|err| format!("open {:?}: {:?}", self.path(), err))?;

        filelog.get_rev_by_nodeid(self.nodeid())
            .map(|node| node.parents().into_iter()
                .map(|p| GQLFile::new(&self.path(), &p))
                .collect())
            .map_err(From::from)
    }
});
