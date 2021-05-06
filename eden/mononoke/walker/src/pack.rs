/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::NodeType;

use blobstore::SizeMetadata;
use mononoke_types::Timestamp;
use scuba_ext::MononokeScubaSampleBuilder;
use std::collections::HashSet;

const RUN_START: &str = "run_start";
const CHUNK_NUM: &str = "chunk_num";
const BLOBSTORE_KEY: &str = "blobstore_key";
const NODE_TYPE: &str = "node_type";
const NODE_FINGERPRINT: &str = "node_fingerprint";
const SIMILARITY_KEY: &str = "similarity_key";
const MTIME: &str = "mtime";
const UNCOMPRESSED_SIZE: &str = "uncompressed_size";
const UNIQUE_COMPRESSED_SIZE: &str = "unique_compressed_size";
const PACK_KEY: &str = "pack_key";
const RELEVANT_UNCOMPRESSED_SIZE: &str = "relevant_uncompressed_size";
const RELEVANT_COMPRESSED_SIZE: &str = "relevant_compressed_size";

/// What do we log for each blobstore key
pub struct PackInfo {
    pub blobstore_key: String,
    pub node_type: NodeType, // Might be different from type implied by blobstore_key's prefix string, e.g. if loading a node does multiple blobstore accesses
    pub node_fingerprint: Option<u64>, // the short hash of the graph level node
    pub similarity_key: Option<u64>, // typically the hash of associated repo path
    pub mtime: Option<u64>, // typically the commit time of Changeset from which this item was reached
    pub uncompressed_size: u64, // How big is the value for this key, in bytes.
    pub sizes: Option<SizeMetadata>,
}

pub trait PackInfoLogger {
    fn log(&self, info: PackInfo);
}

/// What to log for packing and where to send it
#[derive(Clone)]
pub struct PackInfoLogOptions {
    pub log_node_types: HashSet<NodeType>,
    pub log_dest: MononokeScubaSampleBuilder, // TODO(ahornby), add an enum once alternate destinations possible
}

impl PackInfoLogOptions {
    pub fn make_logger(&self, run_start: Timestamp, chunk_num: u64) -> ScubaPackInfoLogger {
        ScubaPackInfoLogger::new(self.log_dest.clone(), run_start, chunk_num)
    }
}

// Used for logging to scuba and to JSON
pub struct ScubaPackInfoLogger {
    scuba: MononokeScubaSampleBuilder,
    run_start: Timestamp, // So we can distinguish runs of the logger. If checkpointing then this is the checkpoint creation timestamp
    chunk_num: u64, // What chunk if is in this sequence from master, lower is closer to master. If not chunking its 1.
}

impl ScubaPackInfoLogger {
    pub fn new(scuba: MononokeScubaSampleBuilder, run_start: Timestamp, chunk_num: u64) -> Self {
        Self {
            scuba,
            run_start,
            chunk_num,
        }
    }
}

impl PackInfoLogger for ScubaPackInfoLogger {
    fn log(&self, info: PackInfo) {
        let mut scuba = self.scuba.clone();
        scuba
            .add(BLOBSTORE_KEY, info.blobstore_key)
            .add(RUN_START, self.run_start.timestamp_seconds())
            .add(CHUNK_NUM, self.chunk_num)
            .add(NODE_TYPE, info.node_type.as_ref())
            .add_opt(NODE_FINGERPRINT, info.node_fingerprint)
            .add_opt(SIMILARITY_KEY, info.similarity_key)
            .add_opt(MTIME, info.mtime)
            .add(UNCOMPRESSED_SIZE, info.uncompressed_size);

        if let Some(sizes) = info.sizes {
            scuba.add(UNIQUE_COMPRESSED_SIZE, sizes.unique_compressed_size);
            if let Some(pack_meta) = sizes.pack_meta {
                scuba.add(PACK_KEY, pack_meta.pack_key);
                scuba.add(
                    RELEVANT_UNCOMPRESSED_SIZE,
                    pack_meta.relevant_uncompressed_size,
                );
                scuba.add(RELEVANT_COMPRESSED_SIZE, pack_meta.relevant_compressed_size);
            }
        }

        scuba.log();
    }
}
