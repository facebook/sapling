// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::Future;

use juniper::FieldResult;

use mercurial_types::{changeset, Changeset, NodeHash};

use manifest::{GQLManifest, GQLPath};
use node::GQLNodeId;
use repo::RepoCtx;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct GQLChangeset(GQLNodeId);

impl From<GQLNodeId> for GQLChangeset {
    fn from(csid: GQLNodeId) -> Self {
        GQLChangeset(csid)
    }
}

impl From<NodeHash> for GQLChangeset {
    fn from(csid: NodeHash) -> Self {
        GQLChangeset(GQLNodeId::from(csid))
    }
}

pub struct GQLTime(changeset::Time);

graphql_object!(GQLTime: RepoCtx as "Time" |&self| {
    description: "Time a commit was made"

    field time() -> i64 as "UTC time in seconds since Unix Epoch" {
        self.0.time as i64
    }

    field tz() -> i64 as "Timezone offset in seconds from UTC" {
        self.0.tz as i64
    }
});

graphql_object!(GQLChangeset: RepoCtx as "Changeset" |&self| {
    description: "A single atomic change"

    field id() -> &GQLNodeId as "changeset identifier" {
        &self.0
    }

    field manifest(&executor) -> FieldResult<GQLManifest>
            as "manifest identifier" {
        executor.context().repo().get_changeset_by_nodeid(&self.0)
                        .wait() // TODO(jsgf) make async
                        .map(|cs| From::from(*cs.manifestid()))
                        .map_err(From::from)
    }

    field user(&executor) -> FieldResult<String>
            as "user" {
        executor.context().repo().get_changeset_by_nodeid(&self.0)
                        .wait() // TODO(jsgf) make async
                        .map(|cs| String::from_utf8_lossy(cs.user()).into_owned())
                        .map_err(From::from)
    }

    field comments(&executor) -> FieldResult<String>
            as "commit comments" {
        executor.context().repo().get_changeset_by_nodeid(&self.0)
                        .wait() // TODO(jsgf) make async
                        .map(|cs| String::from_utf8_lossy(cs.comments()).into_owned())
                        .map_err(From::from)
    }

    field time(&executor) -> FieldResult<GQLTime>
            as "commit time" {
        executor.context().repo().get_changeset_by_nodeid(&self.0)
                        .wait() // TODO(jsgf) make async
                        .map(|cs| *cs.time())
                        .map(GQLTime)
                        .map_err(From::from)
    }

    field paths(&executor) -> FieldResult<Vec<GQLPath>>
            as "paths affected by commit" {
        executor.context().repo().get_changeset_by_nodeid(&self.0)
                        .wait() // TODO(jsgf) make async
                        .map(|cs| cs.files().into_iter().map(GQLPath::from).collect())
                        .map_err(From::from)
    }

    field parents(&executor) -> FieldResult<Vec<GQLChangeset>>
            as "get changeset's parents" {
        executor.context().repo().get_changeset_by_nodeid(&self.0)
                        .wait() // TODO(jsgf) make async
                        .map(|cs| cs.parents().into_iter()
                                        .map(GQLChangeset::from)
                                        .collect())
                        .map_err(From::from)
    }

    //field extra() -> &BTreeMap<String, String> as "extra metadata" {
    //    self.extra()
    //}
});
