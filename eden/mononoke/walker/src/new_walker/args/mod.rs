/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod progress;
pub mod sampling;
pub mod tail_args;
pub mod walk_root;

use anyhow::Error;
use itertools::{process_results, Itertools};
use std::collections::HashSet;
use walker_commands_impl::graph::NodeType;
use walker_commands_impl::setup::{parse_interned_value, parse_node_value};
use walker_commands_impl::state::InternedType;

fn parse_node_types<'a>(
    include_node_types: impl Iterator<Item = &'a String>,
    exclude_node_types: impl Iterator<Item = &'a String>,
    default: &[NodeType],
) -> Result<HashSet<NodeType>, Error> {
    let mut include_node_types = parse_node_values(include_node_types, default)?;
    let exclude_node_types = parse_node_values(exclude_node_types, &[])?;
    include_node_types.retain(|x| !exclude_node_types.contains(x));
    Ok(include_node_types)
}

fn parse_node_values<'a>(
    values: impl Iterator<Item = &'a String>,
    default: &[NodeType],
) -> Result<HashSet<NodeType>, Error> {
    let node_values = process_results(values.map(|x| parse_node_value(x)), |s| s.concat())?;

    if node_values.is_empty() {
        return Ok(HashSet::from_iter(default.iter().cloned()));
    }
    Ok(node_values)
}

fn parse_interned_types<'a>(
    include_types: impl Iterator<Item = &'a String>,
    exclude_types: impl Iterator<Item = &'a String>,
    default: &[InternedType],
) -> Result<HashSet<InternedType>, Error> {
    let mut include_types = parse_interned_values(include_types, default)?;
    let exclude_types = parse_interned_values(exclude_types, &[])?;
    include_types.retain(|x| !exclude_types.contains(x));
    Ok(include_types)
}

fn parse_interned_values<'a>(
    values: impl Iterator<Item = &'a String>,
    default: &[InternedType],
) -> Result<HashSet<InternedType>, Error> {
    let values = process_results(values.map(|v| parse_interned_value(v, default)), |s| {
        s.concat()
    })?;

    if values.is_empty() {
        return Ok(default.iter().cloned().collect());
    }
    Ok(values)
}
