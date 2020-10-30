/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{format_err, Result};

use dag::Id as Vertex;

use mononoke_types::ChangesetId;

#[derive(Debug)]
pub struct MemIdMap {
    vertex2cs: HashMap<Vertex, ChangesetId>,
    cs2vertex: HashMap<ChangesetId, Vertex>,
}

impl MemIdMap {
    pub fn new() -> Self {
        Self {
            vertex2cs: HashMap::new(),
            cs2vertex: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.vertex2cs.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Vertex, ChangesetId)> + '_ {
        self.vertex2cs
            .iter()
            .map(|(&vertex, &cs_id)| (vertex, cs_id))
    }

    pub fn insert(&mut self, vertex: Vertex, cs_id: ChangesetId) {
        self.vertex2cs.insert(vertex, cs_id);
        self.cs2vertex.insert(cs_id, vertex);
    }

    pub fn find_changeset_id(&self, vertex: Vertex) -> Option<ChangesetId> {
        self.vertex2cs.get(&vertex).copied()
    }

    pub fn get_changeset_id(&self, vertex: Vertex) -> Result<ChangesetId> {
        self.find_changeset_id(vertex)
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", vertex))
    }

    pub fn find_vertex(&self, cs_id: ChangesetId) -> Option<Vertex> {
        self.cs2vertex.get(&cs_id).copied()
    }

    pub fn get_vertex(&self, cs_id: ChangesetId) -> Result<Vertex> {
        self.find_vertex(cs_id)
            .ok_or_else(|| format_err!("Failed to find find changeset id {} in IdMap", cs_id))
    }
}
