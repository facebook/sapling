/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context as _, Error};
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::derivable::BonsaiDerivable;
use mercurial_derived_data::MappedHgChangesetId;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use strum::IntoEnumIterator;
use walker_commands_impl::graph::NodeType;

const ALL: &str = "all";
const BONSAI: &str = "bonsai";
pub const DEFAULT: &str = "default";
const DERIVED: &str = "derived";
const HG: &str = "hg";

const DEFAULT_INCLUDE_NODE_TYPES: &[NodeType] = &[
    NodeType::Bookmark,
    NodeType::Changeset,
    NodeType::BonsaiHgMapping,
    NodeType::PhaseMapping,
    NodeType::PublishedBookmarks,
    NodeType::HgBonsaiMapping,
    NodeType::HgChangeset,
    NodeType::HgChangesetViaBonsai,
    NodeType::HgManifest,
    NodeType::HgFileEnvelope,
    NodeType::HgFileNode,
    NodeType::FileContent,
    NodeType::FileContentMetadata,
    NodeType::AliasContentMapping,
];

const BONSAI_NODE_TYPES: &[NodeType] = &[NodeType::Bookmark, NodeType::Changeset];
const HG_DERIVED_TYPES: &[&str] = &[MappedHgChangesetId::NAME, FilenodesOnlyPublic::NAME];

const DERIVED_PREFIX: &str = "derived_";

static DERIVED_DATA_NODE_TYPES: Lazy<HashMap<String, Vec<NodeType>>> = Lazy::new(|| {
    let mut m: HashMap<String, Vec<NodeType>> = HashMap::new();
    for t in NodeType::iter() {
        if let Some(name) = t.derived_data_name() {
            m.entry(format!("{}{}", DERIVED_PREFIX, name))
                .or_default()
                .push(t);
        }
    }
    m
});

#[derive(Debug, Clone)]
pub struct NodeTypeArg(pub Vec<NodeType>);

impl NodeTypeArg {
    pub fn new<'a>(it: impl Iterator<Item = &'a NodeType>) -> Self {
        NodeTypeArg(it.cloned().collect())
    }

    pub fn parse_args(args: &[Self]) -> HashSet<NodeType> {
        HashSet::from_iter(args.iter().flat_map(|arg| arg.0.clone()))
    }

    pub fn filter_nodes(include_nodes: &[Self], exclude_nodes: &[Self]) -> HashSet<NodeType> {
        let mut include_nodes = Self::parse_args(include_nodes);
        let exclude_nodes = Self::parse_args(exclude_nodes);
        include_nodes.retain(|x| !exclude_nodes.contains(x));
        include_nodes
    }
}

impl FromStr for NodeTypeArg {
    type Err = Error;

    fn from_str(arg: &str) -> Result<NodeTypeArg, Error> {
        Ok(match arg {
            ALL => NodeTypeArg(NodeType::iter().collect()),
            BONSAI => NodeTypeArg::new(BONSAI_NODE_TYPES.iter()),
            DEFAULT => NodeTypeArg::new(DEFAULT_INCLUDE_NODE_TYPES.iter()),
            DERIVED => NodeTypeArg::new(DERIVED_DATA_NODE_TYPES.values().flatten()),
            HG => {
                let mut node_types = vec![];
                for hg_derived in HG_DERIVED_TYPES {
                    let hg_derived = format!("{}{}", DERIVED_PREFIX, hg_derived);
                    let nodes_derived = DERIVED_DATA_NODE_TYPES.get(&hg_derived);
                    if let Some(nd) = nodes_derived {
                        nd.iter().for_each(|node| node_types.push(node.clone()));
                    }
                }
                NodeTypeArg(node_types)
            }
            _ => {
                if let Some(node_types) = DERIVED_DATA_NODE_TYPES.get(arg) {
                    NodeTypeArg(node_types.clone())
                } else {
                    NodeType::from_str(arg)
                        .map(|e| NodeTypeArg(vec![e]))
                        .with_context(|| format_err!("Unknown NodeType {}", arg))?
                }
            }
        })
    }
}
