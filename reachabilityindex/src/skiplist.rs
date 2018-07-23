// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use chashmap::CHashMap;

use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mononoke_types::Generation;

const DEFAULT_EDGE_COUNT: u32 = 10;

// Each indexed node fits into one of two categories:
// - It has skiplist edges
// - It only has edges to its parents.
enum SkiplistNodeType {
    _SkipEdges(Vec<(Generation, HgNodeHash)>),
    _ParentEdges(Vec<(Generation, HgNodeHash)>),
}

/// Structure for indexing skip list edges for reachability queries.
pub struct SkiplistIndex {
    _repo: Arc<BlobRepo>,

    // Each hash that the structure knows about is mapped to a  collection
    // of (Gen, Hash) pairs, wrapped in an enum. The semantics behind this are:
    // - If the hash isn't in the hash map, the node hasn't been indexed yet.
    // - If the enum type is SkipEdges, then we can safely traverse the longest
    //   edge that doesn't pass the generation number of the destination.
    // - If the enum type is ParentEdges, then we couldn't safely add skip edges
    //   from this node (which is always the case for a merge node), so we must
    //   recurse on all the children.
    _skip_list_edges: Arc<CHashMap<HgNodeHash, SkiplistNodeType>>,
    skip_edges_per_node: u32,
}

impl SkiplistIndex {
    pub fn new(repo: Arc<BlobRepo>) -> Self {
        SkiplistIndex {
            _repo: repo,
            _skip_list_edges: Arc::new(CHashMap::new()),
            skip_edges_per_node: DEFAULT_EDGE_COUNT,
        }
    }

    pub fn with_skip_edge_count(self, skip_edges_per_node: u32) -> Self {
        SkiplistIndex {
            skip_edges_per_node,
            ..self
        }
    }

    pub fn skip_edge_count(&self) -> u32 {
        self.skip_edges_per_node
    }
}

#[cfg(test)]
mod test {
    use async_unit;
    use chashmap::CHashMap;
    use std::sync::Arc;

    use super::*;
    use linear;

    #[test]
    fn simple_init() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new(repo.clone());
            assert_eq!(sli.skip_edge_count(), DEFAULT_EDGE_COUNT);

            let sli_with_20 = SkiplistIndex::new(repo.clone()).with_skip_edge_count(20);
            assert_eq!(sli_with_20.skip_edge_count(), 20);
        });
    }

    #[test]
    fn arc_chash_is_sync_and_send() {
        fn is_sync<T: Sync>() {}
        fn is_send<T: Send>() {}

        is_sync::<Arc<CHashMap<HgNodeHash, SkiplistNodeType>>>();
        is_send::<Arc<CHashMap<HgNodeHash, SkiplistNodeType>>>();
    }
}
