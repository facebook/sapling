// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::ops::Deref;

use futures::Future;

use juniper::{FieldResult, Value};

use mercurial_types::{NodeHash, Path};

use repo::RepoCtx;
use node::GQLNodeId;
use manifestobj::GQLManifestObj;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct GQLPath(Path);

impl GQLPath {
    pub fn new(path: Path) -> GQLPath {
        GQLPath(path)
    }
}

impl<'a> From<&'a Path> for GQLPath {
    fn from(path: &'a Path) -> Self {
        GQLPath::new(path.clone())
    }
}

impl Deref for GQLPath {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

graphql_scalar!(GQLPath {
    description: "A path in the manifest"

    resolve(&self) -> Value {
        let path = &self.0;
        Value::string(String::from_utf8_lossy(path.to_vec().as_slice()).into_owned())
    }

    from_input_value(v: &InputValue) -> Option<GQLPath> {
        v.as_string_value()
            .map(|s| Path::new(s.as_bytes()))
            .and_then(Result::ok)
            .map(GQLPath::new)
    }
});


pub struct GQLManifest(GQLNodeId);

impl From<NodeHash> for GQLManifest {
    fn from(id: NodeHash) -> Self {
        GQLManifest(GQLNodeId::from(id))
    }
}

graphql_object!(GQLManifest: RepoCtx as "Manifest" |&self| {
    description: "The manifest for a specific changeset"

    field id() -> &GQLNodeId as "Get manifest id" {
        &self.0
    }

    // Return just a list of paths
    field paths(&executor) -> FieldResult<Vec<GQLPath>> as "Get paths" {
        let mut repo = executor.context().repo();
        repo.get_manifest_by_nodeid(&self.0)
            .wait() // TODO(jsgf) make async
            .map(|m| m.manifest().into_iter()
                .map(|(path, _)| GQLPath::from(path))
                .collect())
            .map_err(From::from)
    }

    // Return a list of manifest objects
    field entries(&executor) -> FieldResult<Vec<GQLManifestObj>> as "Get entries" {
        let mut repo = executor.context().repo();
        repo.get_manifest_by_nodeid(&self.0)
            .wait() // TODO(jsgf) make async
            .map(|m| m.manifest().into_iter()
                .map(|(path, details)| GQLManifestObj::new(&From::from(path), details))
                .collect())
            .map_err(From::from)
    }

    // Look up a specific path (XXX fileset)
    field lookup(&executor, path: GQLPath) -> FieldResult<GQLManifestObj> as "Lookup entry" {
        let mut repo = executor.context().repo();
        let manifest = repo.get_manifest_by_nodeid(&self.0)
            .wait()?; // TODO(jsgf) make async
        let details = manifest.lookup(&path)
                .ok_or(format!("\"{}\" not present", *path))?;
        Ok(GQLManifestObj::new(&path, details))
    }
});
