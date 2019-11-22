/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::Node;
use crate::walk::NodeChecker;
use mercurial_types::{HgFileNodeId, HgManifestId};
use mononoke_types::{ChangesetId, ContentId};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug)]
pub struct WalkStateArcMutex {
    visited_bcs: Arc<Mutex<HashSet<ChangesetId>>>,
    visited_file: Arc<Mutex<HashSet<ContentId>>>,
    visited_hg_file: Arc<Mutex<HashSet<HgFileNodeId>>>,
    visited_hg_manifest: Arc<Mutex<HashSet<HgManifestId>>>,
}

impl WalkStateArcMutex {
    pub fn new() -> Self {
        Self {
            visited_bcs: Arc::new(Mutex::new(HashSet::new())),
            visited_file: Arc::new(Mutex::new(HashSet::new())),
            visited_hg_file: Arc::new(Mutex::new(HashSet::new())),
            visited_hg_manifest: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

impl NodeChecker for WalkStateArcMutex {
    fn has_visited(self: &Self, n: &Node) -> bool {
        match n {
            Node::BonsaiChangeset(bcs_id) => {
                let visited_bcs = self.visited_bcs.lock().unwrap();
                visited_bcs.contains(bcs_id)
            }
            Node::FileContent(content_id) => {
                let visited_file = self.visited_file.lock().unwrap();
                visited_file.contains(content_id)
            }
            Node::HgFileEnvelope(id) => {
                let visited_hg_file = self.visited_hg_file.lock().unwrap();
                visited_hg_file.contains(id)
            }
            Node::HgFileNode((_path, id)) => {
                let visited_hg_file = self.visited_hg_file.lock().unwrap();
                visited_hg_file.contains(id)
            }
            Node::HgManifest((_path, id)) => {
                let visited_hg_manifest = self.visited_hg_manifest.lock().unwrap();
                visited_hg_manifest.contains(id)
            }
            _ => false,
        }
    }

    fn record_visit(self: &mut Self, n: &Node) -> bool {
        match n {
            Node::BonsaiChangeset(bcs_id) => {
                let mut visited_bcs = self.visited_bcs.lock().unwrap();
                visited_bcs.insert(*bcs_id)
            }
            Node::FileContent(content_id) => {
                let mut visited_file = self.visited_file.lock().unwrap();
                visited_file.insert(*content_id)
            }
            Node::HgFileEnvelope(id) => {
                let mut visited_hg_file = self.visited_hg_file.lock().unwrap();
                visited_hg_file.insert(*id)
            }
            Node::HgFileNode((_path, id)) => {
                let mut visited_hg_file = self.visited_hg_file.lock().unwrap();
                visited_hg_file.insert(*id)
            }
            Node::HgManifest((_path, id)) => {
                let mut visited_hg_manifest = self.visited_hg_manifest.lock().unwrap();
                visited_hg_manifest.insert(*id)
            }
            _ => true,
        }
    }
}
